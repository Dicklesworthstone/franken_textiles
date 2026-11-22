#!/bin/sh
# After `bun run deploy`: confirm the published site answers the way the configuration intends.
# Usage: sh scripts/check-deploy.sh [https://franken-textiles.com]
set -u
site="${1:-https://franken-textiles.com}"
host="${site#https://}"
agent="OpenAI File Downloader, XaiImageApiFetch/1.0"
failed=0
check() { # description, expected, actual
  if [ "$2" = "$3" ]; then echo "ok    $1"; else echo "FAIL  $1: expected '$2', got '$3'"; failed=1; fi
}
code() { curl -s -o /dev/null -A "$agent" -m 30 -w '%{http_code}' "$@"; }
header() { curl -s -D - -o /dev/null -A "$agent" -m 30 "$2" | tr -d '\r' | awk -F': ' -v h="$1" 'tolower($1)==h{print $2; exit}'; }

for path in / /how-it-works/ /get-started/ /wardrobe/sundress/ /wardrobe/sundress/redraft/ /wardrobe/hussar/ /studies/trousers/ /experiments/strip/ \
  /wardrobe/hussar/embroidery/ /studies/tiling/ /studies/sewing/ /studies/wrap-skirt/ /experiments/wash/ /experiments/thermal/ /experiments/friction/ \
  /sections/repair/ /workbench/ /workbench/trace /robots.txt /sitemap.xml /manifest.webmanifest /favicon.ico; do
  check "200 $path" 200 "$(code "$site$path")"
done
check "404 for a missing page" 404 "$(code "$site/no-such-page/")"
check "http redirects to https" 301 "$(code "http://$host/")"
check "www redirects to the bare domain" "$site/get-started/" "$(curl -s -o /dev/null -A "$agent" -m 30 -w '%{redirect_url}' "https://www.$host/get-started/")"
check "films answer byte-range requests (needed by Safari)" 206 "$(code -H 'Range: bytes=0-99' "$site/media/bolero.mp4")"
check "wasm content type" "application/wasm" "$(header content-type "$site/workbench/pkg/ftx_wasm_bg.wasm")"
case "$(header content-security-policy "$site/")" in *"default-src 'self'"*) echo "ok    content security policy";; *) echo "FAIL  content security policy missing"; failed=1;; esac
chunk="$(curl -s -A "$agent" -m 30 "$site/" | grep -o '/_next/static/chunks/[A-Za-z0-9_.-]*\.js' | head -1)"
case "$(header cache-control "$site$chunk")" in *immutable*) echo "ok    hashed assets cached immutably";; *) echo "FAIL  hashed assets not immutable ($chunk)"; failed=1;; esac
case "$(curl -s -A "$agent" -m 30 "$site/")" in *"rel=\"canonical\" href=\"$site/\""*) echo "ok    canonical names $site";; *) echo "FAIL  canonical does not name $site"; failed=1;; esac
exit $failed
