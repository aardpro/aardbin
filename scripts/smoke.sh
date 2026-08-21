#!/usr/bin/env bash
# aardbin integration smoke test — covers SPEC acceptance criteria AC-01..AC-14.
# Requires: curl, python3, and a built target/debug/aardbin binary.
set -u

cd "$(dirname "$0")/.." || exit 1

PORT=18091
BASE="http://127.0.0.1:$PORT"
TMP="$(mktemp -d)"
DATA_DIR="$TMP/data"
JAR="$TMP/cookies.txt"
H="$TMP/headers.txt"
LOG="$TMP/server.log"
SSE_OUT="$TMP/sse.out"
BIN=./target/debug/aardbin
CLI=./target/debug/aardbin-cli

ACCESS_KEY="smoke-access-key-0123456789"
CRYPTO_KEY="$(python3 -c 'import secrets;print(secrets.token_hex(32))')"
WRONG_KEY="$(python3 -c 'import secrets;print(secrets.token_hex(32))')"
COOKIE_SECURE=false
MAX_CONTENT_BYTES=1048576

PASS=0
FAIL=0

check() { # check <name> <0|1>
  if [ "$2" -eq 0 ]; then
    printf '  PASS  %s\n' "$1"; PASS=$((PASS + 1))
  else
    printf '  FAIL  %s\n' "$1"; FAIL=$((FAIL + 1))
  fi
}

code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }

start_server() {
  ACCESS_KEY="$ACCESS_KEY" CRYPTO_KEY="$CRYPTO_KEY" \
  LISTEN_ADDR="127.0.0.1:$PORT" DATA_DIR="$DATA_DIR" PAGE_SIZE=5 \
  COOKIE_SECURE="$COOKIE_SECURE" MAX_CONTENT_BYTES="$MAX_CONTENT_BYTES" \
  RUST_LOG=aardbin=info "$BIN" >>"$LOG" 2>&1 &
  SRV=$!
  local i
  for i in $(seq 1 100); do
    if [ "$(code "$BASE/healthz")" = "200" ]; then return 0; fi
    sleep 0.1
  done
  echo "  FATAL: server did not start"; tail -20 "$LOG"; exit 1
}

stop_server() { kill "$SRV" 2>/dev/null; wait "$SRV" 2>/dev/null; }

mkdir -p "$DATA_DIR"

echo "== AC-14 / AC-01 / AC-12 : health, auth, ratelimit, origin =="
start_server

# healthz without auth
[ "$(code "$BASE/healthz")" = "200" ]; check "healthz returns 200 unauthenticated" $?

# unauthenticated GET / redirects to /login
curl -s -o /dev/null -D "$H" "$BASE/"
grep -qi "location: /login" "$H"; check "GET / without session -> 303 /login" $?

# login page: no localStorage usage (AC-01)
LOGIN_HTML="$(curl -s "$BASE/login")"
echo "$LOGIN_HTML" | grep -q "Access Key"; check "login page renders" $?
echo "$LOGIN_HTML" | grep -qi "localStorage" && LS=1 || LS=0
[ "$LS" = "0" ]; check "ACCESS_KEY never touches localStorage" $?

# wrong access key -> 303 error
curl -s -o /dev/null -D "$H" -X POST --data-urlencode "access_key=wrong" "$BASE/login"
grep -qi "location: /login?error=1" "$H"; check "wrong ACCESS_KEY rejected" $?

# rate limit: 5 failures then 429 (AC-12)
RATE_FAIL=1
for i in 2 3 4 5; do
  curl -s -o /dev/null -X POST --data-urlencode "access_key=wrong" "$BASE/login"
done
if [ "$(code -X POST --data-urlencode "access_key=wrong" "$BASE/login")" = "429" ]; then RATE_FAIL=0; fi
check "login rate-limited after 5 failures (429)" $RATE_FAIL

# origin guard (AC-12): cross-site POST rejected
[ "$(code -X POST -H 'Sec-Fetch-Site: cross-site' --data-urlencode "access_key=x" "$BASE/login")" = "403" ]; check "Sec-Fetch-Site cross-site POST -> 403" $?
[ "$(code -X POST -H 'Origin: http://evil.example' --data-urlencode "access_key=x" "$BASE/login")" = "403" ]; check "mismatched Origin POST -> 403" $?
[ "$(code -X POST -H "Origin: $BASE" --data-urlencode "access_key=x" "$BASE/login")" != "403" ]; check "same-origin POST allowed" $?

stop_server

# restart clears the in-memory limiter; also proves stateless sessions later
echo "== AC-01 : successful login, session cookie flags =="
start_server
curl -s -o /dev/null -D "$H" -c "$JAR" -X POST --data-urlencode "access_key=$ACCESS_KEY" "$BASE/login"
grep -qiE "location: /\s*$" "$H"; check "correct ACCESS_KEY -> 303 /" $?
grep -qi "set-cookie: aardbin_session=" "$H"; check "session cookie issued" $?
grep -qi "httponly" "$H"; check "session cookie is HttpOnly" $?
grep -qi "samesite=lax" "$H"; check "session cookie SameSite=Lax" $?
grep -qi "max-age=" "$H"; check "session cookie has Max-Age" $?
grep -qi "secure" "$H" && S=1 || S=0
[ "$COOKIE_SECURE" = "true" ] && { [ "$S" = "1" ]; check "Secure flag present (COOKIE_SECURE=true)" $?; } \
  || { [ "$S" = "0" ]; check "Secure flag absent (COOKIE_SECURE=false)" $?; }

# authenticated landing page
[ "$(code -b "$JAR" "$BASE/")" = "200" ]; check "authenticated GET / -> 200" $?
stop_server

# restart — session must survive (stateless HMAC, SPEC §7.2.1)
start_server
[ "$(code -b "$JAR" "$BASE/")" = "200" ]; check "session survives server restart" $?

echo "== AC-02 / AC-03 / AC-04 / AC-05 : create, encrypt, copy, download =="
printf 'the secret content marker\nsecond line\n' > "$TMP/plain.txt"
curl -s -o /dev/null -D "$H" -b "$JAR" -X POST \
  -F "title=Encryption Test" -F "content=the secret content marker" -F "files=@$TMP/plain.txt" "$BASE/records"
[ "$(head -1 "$H" | awk '{print $2}')" = "204" ]; check "POST /records -> 204" $?
grep -qi "hx-redirect: /" "$H"; check "create returns HX-Redirect /" $?

LIST_HTML="$(curl -s -b "$JAR" "$BASE/")"
echo "$LIST_HTML" | grep -q "Encryption Test"; check "record title appears in list" $?

# AC-03: plaintext not in the DB file
python3 - "$DATA_DIR/aardbin.db" <<'PY'
import sys
data = open(sys.argv[1], 'rb').read()
marker = b"the secret content marker"
sys.exit(0 if marker not in data else 1)
PY
check "content not stored in plaintext (AES-GCM at rest)" $?

# AC-02: attachment landed on disk as a UUID file
ATT_COUNT=$(ls "$DATA_DIR/attachments" | wc -l)
[ "$ATT_COUNT" -eq 1 ]; check "attachment file written to data/attachments" $?

# AC-04: copy endpoint returns exact decrypted content
RID=$(echo "$LIST_HTML" | grep -o '/records/[a-f0-9-]*/edit' | head -1 | cut -d/ -f3)
[ -n "$RID" ]; check "record id discoverable" $?
COPY="$(curl -s -b "$JAR" "$BASE/records/$RID/copy")"
[ "$COPY" = "the secret content marker" ]; check "copy returns exact content" $?

# AC-05: download bytes equal upload; headers correct
AID=$(ls "$DATA_DIR/attachments" | head -1)
curl -s -D "$H" -o "$TMP/dl.txt" -b "$JAR" "$BASE/attachments/$AID"
cmp -s "$TMP/plain.txt" "$TMP/dl.txt"; check "downloaded bytes == uploaded bytes" $?
grep -qi "x-content-type-options: nosniff" "$H"; check "nosniff header present" $?
grep -qi "content-length: $(wc -c < "$TMP/plain.txt")" "$H"; check "content-length correct" $?

echo "== AC-12 : attachment disposition policy, unicode filenames =="
python3 -c "import base64,sys; open(sys.argv[1],'wb').write(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M8AAAMBAQDJ/pLvAAAAAElFTkSuQmCC'))" "$TMP/pixel.png"
printf '<script>alert(1)</script>' > "$TMP/x.html"
printf 'unicode content' > "$TMP/截图.txt"

# upload html + png + unicode-named file onto a new record
curl -s -o /dev/null -b "$JAR" -X POST -F "title=File Policy" \
  -F "files=@$TMP/x.html;type=text/html" \
  -F "files=@$TMP/pixel.png;type=image/png" \
  -F "files=@$TMP/截图.txt" "$BASE/records"
HTML_AID=$(python3 - "$DATA_DIR/aardbin.db" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1]); r=c.execute("select id from attachments where original_filename='x.html'").fetchone(); print(r[0] if r else '')
PY
)
curl -s -D "$H" -o /dev/null -b "$JAR" "$BASE/attachments/$HTML_AID?inline=1"
grep -qi 'content-disposition: attachment' "$H"; check "HTML attachment forced to download even with ?inline=1" $?

PNG_AID=$(python3 - "$DATA_DIR/aardbin.db" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1]); r=c.execute("select id from attachments where original_filename='pixel.png'").fetchone(); print(r[0] if r else '')
PY
)
curl -s -D "$H" -o /dev/null -b "$JAR" "$BASE/attachments/$PNG_AID?inline=1"
grep -qi 'content-disposition: inline' "$H"; check "whitelisted image inlines with ?inline=1" $?

UNI_AID=$(python3 - "$DATA_DIR/aardbin.db" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1]); r=c.execute("select id from attachments where original_filename='截图.txt'").fetchone(); print(r[0] if r else '')
PY
)
curl -s -D "$H" -o /dev/null -b "$JAR" "$BASE/attachments/$UNI_AID"
grep -qi "filename\*=UTF-8''" "$H"; check "unicode filename uses RFC5987 filename*" $?
grep -qi "%E6%88%AA" "$H"; check "unicode filename percent-encoded" $?

echo "== AC-10 / AC-09 : delete record & attachment cascade =="
# delete attachment (edit-page action)
curl -s -D "$H" -o /dev/null -b "$JAR" -X POST "$BASE/records/$RID/attachments/$AID/delete"
[ "$(head -1 "$H" | awk '{print $2}')" = "200" ]; check "attachment delete -> 200" $?
[ ! -f "$DATA_DIR/attachments/$AID" ]; check "attachment file removed from disk" $?
[ "$(code -b "$JAR" "$BASE/attachments/$AID")" = "404" ]; check "deleted attachment 404 on download" $?

# delete the record → record + attachments gone
curl -s -o /dev/null -D "$H" -b "$JAR" -X POST "$BASE/records/$RID/delete"
[ "$(head -1 "$H" | awk '{print $2}')" = "200" ]; check "record delete -> 200" $?
[ "$(code -b "$JAR" "$BASE/records/$RID/copy")" = "404" ]; check "record gone after delete" $?

echo "== AC-08 : pagination (PAGE_SIZE=5), ordering, title fallback =="
TOTAL=13
for i in $(seq 1 $((TOTAL - 1))); do
  curl -s -o /dev/null -b "$JAR" -X POST -F "title=bulk-$i" -F "content=body-$i" "$BASE/records"
done
sleep 1
curl -s -o /dev/null -b "$JAR" -X POST -F "title=newest-marker" -F "content=body-newest" "$BASE/records"

P1="$(curl -s -b "$JAR" "$BASE/")"
N1=$(echo "$P1" | grep -o '<article' | wc -l)
[ "$N1" -eq 5 ]; check "page 1 shows 5 records (PAGE_SIZE=5)" $?
echo "$P1" | grep -q "newest-marker"; check "newest record appears first page" $?

P3="$(curl -s -b "$JAR" "$BASE/records?page=3")"
N3=$(echo "$P3" | grep -o '<article' | wc -l)
[ "$N3" -eq 4 ]; check "page 3 shows remaining records" $?
echo "$P3" | grep -q "bulk-"; check "page 3 contains older records" $?

# title fallback (SPEC §10.3)
curl -s -o /dev/null -b "$JAR" -X POST -F "title=" -F "content=fallback-line-marker
second" "$BASE/records"
curl -s -o /dev/null -b "$JAR" -X POST -F "title=" -F "content=" "$BASE/records"
FALLBACK="$(curl -s -b "$JAR" "$BASE/records?page=1")"
echo "$FALLBACK" | grep -q "fallback-line-marker"; check "empty title -> first content line" $?
echo "$FALLBACK" | grep -q "Untitled"; check "empty record -> Untitled" $?

echo "== AC-14 : SSE data_changed + heartbeat =="
# The heartbeat interval is 25s; allow up to 30s for the curl to collect it.
curl -s -N -b "$JAR" --max-time 30 "$BASE/events" > "$SSE_OUT" &
SSE_PID=$!
sleep 1
curl -s -o /dev/null -b "$JAR" -X POST -F "content=sse-trigger-marker" "$BASE/records"
# Wait for curl to finish (up to 30s); kill if still running after timeout.
wait "$SSE_PID" 2>/dev/null || true
grep -q "event: data_changed" "$SSE_OUT"; check "SSE receives data_changed event" $?
grep -q ": ping" "$SSE_OUT"; check "SSE heartbeat : ping received within 30s" $?

echo "== AC-13 : decryption failure degrades gracefully =="
stop_server
CRYPTO_KEY="$WRONG_KEY" start_server
BAD="$(curl -s -b "$JAR" "$BASE/")"
echo "$BAD" | grep -q "Unable to decrypt"; check "wrong CRYPTO_KEY -> 'Unable to decrypt' placeholder" $?
echo "$BAD" | grep -q 'data-copy' && C=1 || C=0
[ "$C" = "0" ]; check "undecryptable record has no Copy button" $?
# attachment of an undecryptable record still downloadable
[ "$(code -b "$JAR" "$BASE/attachments/$UNI_AID")" = "200" ]; check "attachment still downloadable when record undecryptable" $?
stop_server

echo "== AC-11 : orphan files never exposed, logged on scan =="
ORPHAN_ID="11111111-2222-3333-4444-555555555555"
touch "$DATA_DIR/attachments/$ORPHAN_ID"
CRYPTO_KEY="$(python3 -c 'import secrets;print(secrets.token_hex(32))')" # any key; records already unreadable anyway
start_server
[ "$(code -b "$JAR" "$BASE/attachments/$ORPHAN_ID")" = "404" ]; check "orphan file not exposed (404)" $?
grep -q "orphan attachment detected" "$LOG"; check "orphan scan logs a warning" $?
stop_server

echo "== AC-12 : request/attachment size limits =="
MAX_CONTENT_BYTES=64 start_server
BIG=$(python3 -c 'print("x"*500)')
[ "$(code -b "$JAR" -X POST -F "content=$BIG" "$BASE/records")" = "422" ]; check "oversized content rejected (422)" $?
MAX_CONTENT_BYTES=1048576
stop_server

start_server
dd if=/dev/zero of="$TMP/big.bin" bs=1048576 count=3 2>/dev/null
[ "$(code -b "$JAR" -X POST -F "files=@$TMP/big.bin" "$BASE/records")" = "422" ]; check "attachment over 2MiB rejected (422)" $?

echo "== AC-12 : COOKIE_SECURE=true mode =="
stop_server
COOKIE_SECURE=true start_server
curl -s -o /dev/null -D "$H" -c "$TMP/jar2" -X POST --data-urlencode "access_key=$ACCESS_KEY" "$BASE/login"
grep -qi "secure" "$H"; check "COOKIE_SECURE=true sets Secure flag" $?
stop_server
COOKIE_SECURE=false

echo "== logout clears cookie (AC-01) =="
start_server
curl -s -o /dev/null -D "$H" -b "$JAR" -c "$JAR" -X POST "$BASE/logout"
grep -qi "max-age=0" "$H"; check "logout sets Max-Age=0" $?
# fresh request without cookie -> back to login
curl -s -o /dev/null -D "$H" "$BASE/"
grep -qi "location: /login" "$H"; check "no cookie after logout -> 303 /login" $?
stop_server

echo "== CLI end-to-end (E3, E11) =="
# Build CLI if needed
cargo build -p aardbin-cli 2>/dev/null
start_server

# paste: create a record
CLI_OUT=$(env AARDBIN_URL="$BASE" AARDBIN_ACCESS_KEY="$ACCESS_KEY" $CLI paste -t "CLI Test" -c "hello from cli smoke test" 2>&1)
CLI_RC=$?
[ "$CLI_RC" = "0" ]; check "cli paste exits 0" $?
CLI_RID=$(echo "$CLI_OUT" | tail -1)
[ -n "$CLI_RID" ]; check "cli paste returns record id" $?

# list: should show at least 1 record
CLI_OUT=$(env AARDBIN_URL="$BASE" AARDBIN_ACCESS_KEY="$ACCESS_KEY" $CLI list 2>&1)
CLI_RC=$?
[ "$CLI_RC" = "0" ]; check "cli list exits 0" $?
echo "$CLI_OUT" | grep -q "CLI Test"; check "cli list shows created record" $?

# get: fetch the record
CLI_OUT=$(env AARDBIN_URL="$BASE" AARDBIN_ACCESS_KEY="$ACCESS_KEY" $CLI get "$CLI_RID" 2>&1)
CLI_RC=$?
[ "$CLI_RC" = "0" ]; check "cli get exits 0" $?
echo "$CLI_OUT" | grep -q "hello from cli smoke test"; check "cli get shows content" $?
echo "$CLI_OUT" | grep -q "Title: CLI Test"; check "cli get shows title" $?

# upload an attachment via paste with file
printf 'cli attachment content' > "$TMP/cli_att.txt"
CLI_OUT=$(env AARDBIN_URL="$BASE" AARDBIN_ACCESS_KEY="$ACCESS_KEY" $CLI paste -t "CLI With File" -c "has attachment" -f "$TMP/cli_att.txt" 2>&1)
CLI_RC=$?
[ "$CLI_RC" = "0" ]; check "cli paste with file exits 0" $?
CLI_RID2=$(echo "$CLI_OUT" | tail -1)
CLI_OUT=$(env AARDBIN_URL="$BASE" AARDBIN_ACCESS_KEY="$ACCESS_KEY" $CLI get "$CLI_RID2" 2>&1)
echo "$CLI_OUT" | grep -q "cli_att.txt"; check "cli get shows attachment filename" $?

# delete the first record
CLI_OUT=$(env AARDBIN_URL="$BASE" AARDBIN_ACCESS_KEY="$ACCESS_KEY" $CLI delete "$CLI_RID" 2>&1)
CLI_RC=$?
[ "$CLI_RC" = "0" ]; check "cli delete exits 0" $?
echo "$CLI_OUT" | grep -q "Deleted"; check "cli delete confirms" $?

# verify deleted
CLI_OUT=$(env AARDBIN_URL="$BASE" AARDBIN_ACCESS_KEY="$ACCESS_KEY" $CLI get "$CLI_RID" 2>&1)
[ "$?" != "0" ]; check "cli get deleted record fails" $?

# wrong key rejected
CLI_OUT=$(env AARDBIN_URL="$BASE" AARDBIN_ACCESS_KEY="wrong-key-wrong-key1" $CLI list 2>&1)
[ "$?" != "0" ]; check "cli wrong key rejected" $?

stop_server

echo
echo "================================================"
echo "  PASS: $PASS   FAIL: $FAIL"
echo "================================================"
rm -rf "$TMP"
[ "$FAIL" -eq 0 ]
