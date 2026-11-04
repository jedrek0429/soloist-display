#!/bin/sh
set -eu

dist=$(CDPATH= cd -- "$1" && pwd)
key=$2
manager="$dist/soloist-display-x86_64-unknown-linux-musl"
temp=$(mktemp -d)
server=
cleanup() {
  if [ -n "$server" ]; then
    kill "$server" 2>/dev/null || :
    wait "$server" 2>/dev/null || :
  fi
  rm -rf "$temp"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

node - "$dist" "$temp/port" <<'JS' &
const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');
const directory = process.argv[2];
const portFile = process.argv[3];
const server = http.createServer((request, response) => {
  const name = new URL(request.url, 'http://localhost').pathname.slice(1);
  if (!/^[a-zA-Z0-9_.-]+$/.test(name)) {
    response.writeHead(400).end();
    return;
  }
  const source = path.join(directory, name);
  const stream = fs.createReadStream(source);
  stream.on('error', () => {
    if (!response.headersSent) response.writeHead(404).end();
  });
  stream.pipe(response);
});
server.listen(0, '127.0.0.1', () => {
  fs.writeFileSync(portFile, String(server.address().port));
});
JS
server=$!

attempt=0
until [ -s "$temp/port" ]; do
  kill -0 "$server" 2>/dev/null || { echo 'Test asset server failed' >&2; exit 1; }
  attempt=$((attempt + 1))
  [ "$attempt" -lt 100 ] || { echo 'Test asset server timed out' >&2; exit 1; }
  sleep 0.1
done
port=$(cat "$temp/port")

node - "$dist/release-manifest.json" "$temp" "$port" <<'JS'
const fs = require('node:fs');
const path = require('node:path');
const manifest = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const directory = process.argv[3];
const port = process.argv[4];
const match = /^(\d+)\.(\d+)\.(\d+)/.exec(manifest.version);
if (!match) throw new Error('Invalid release version');
const initial = manifest.version;
const updated = `${match[1]}.${match[2]}.${Number(match[3]) + 1}-ci.1`;
for (const artifact of manifest.artifacts) {
  artifact.url = `http://127.0.0.1:${port}/${artifact.name}`;
  artifact.fallback_url = null;
}
for (const [name, version] of [['initial', initial], ['updated', updated]]) {
  const candidate = { ...manifest, version };
  fs.writeFileSync(path.join(directory, `${name}.json`), JSON.stringify(candidate) + '\n');
}
JS

for stage in initial updated; do
  openssl pkeyutl -sign -inkey "$key" -rawin \
    -in "$temp/$stage.json" -out "$temp/$stage.sig.bin"
  openssl base64 -A -in "$temp/$stage.sig.bin" -out "$temp/$stage.sig"
  printf '\n' >> "$temp/$stage.sig"
done

mkdir -p "$temp/home"
export HOME="$temp/home"
export SOLOIST_DISPLAY_TEST_ROOT="$temp/root"
# Use the validated release-time Spotify build only in isolated test mode.
export SOLOIST_DISPLAY_TEST_SOLOIST_ARCHIVE="$dist/soloist_release_x86_64.tar.gz"
export SOLOIST_NAME="Soloist CI"
export SOLOIST_API_KEY="ci-fixture"
export SOLOIST_DISPLAY_MANIFEST="$temp/initial.json"
export SOLOIST_DISPLAY_MANIFEST_SIGNATURE="$temp/initial.sig"

state="$temp/root/state/soloist-display/install.json"
current="$temp/root/data/soloist-display/current"

"$manager" --yes --quiet install
test -f "$state"
test -L "$current"
node - "$state" "$temp/initial.json" <<'JS'
const fs = require('node:fs');
const state = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const manifest = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
if (state.version !== manifest.version) throw new Error('Install version mismatch');
JS

printf 'corrupt\n' >> "$current/bin/soloist-display"
"$manager" --quiet repair
cmp "$current/bin/soloist-display" "$manager"

export SOLOIST_DISPLAY_MANIFEST="$temp/updated.json"
export SOLOIST_DISPLAY_MANIFEST_SIGNATURE="$temp/updated.sig"
"$manager" --quiet update
node - "$state" "$temp/updated.json" <<'JS'
const fs = require('node:fs');
const state = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const manifest = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
if (state.version !== manifest.version) throw new Error('Update version mismatch');
JS

"$manager" --yes --quiet uninstall --soloist remove --data remove
test ! -e "$state"
test ! -e "$current"
test ! -L "$current"
test ! -e "$temp/root/config/systemd/user/soloist-display.service"
echo 'Signed candidate lifecycle verified: install, repair, update, uninstall'
