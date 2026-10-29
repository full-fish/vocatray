#!/usr/bin/env bash
# GitHub 릴리스 올리기 — 빌드가 끝난 뒤에 쓴다 (PLAN.md Phase 5).
#
#   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/vocatray.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
#   npm run tauri build
#   bash scripts/release.sh <바뀐 점을 적은 마크다운 파일>
#
# 버전은 src-tauri/tauri.conf.json에서 읽는다. 올리는 파일 셋:
#   VocaTray_<버전>_x64-setup.exe  — 버전 이름 (latest.json이 가리킨다)
#   VocaTray-setup.exe             — 같은 파일, 홈페이지의 releases/latest/download 링크용
#   latest.json                    — 설치된 앱의 자동 업데이트가 읽는다
# 인증은 git이 쓰는 GitHub 로그인(Git Credential Manager)을 그대로 쓴다.
set -euo pipefail
cd "$(dirname "$0")/.."

NOTES=${1:?"바뀐 점을 적은 마크다운 파일을 주세요"}
REPO=full-fish/vocatray
VERSION=$(node -p 'require("./src-tauri/tauri.conf.json").version')
NAME="VocaTray_${VERSION}_x64-setup.exe"
EXE="src-tauri/target/release/bundle/nsis/$NAME"
[ -f "$EXE.sig" ] || { echo "$EXE.sig 가 없음 — 서명 키를 넣고 빌드했는지 확인"; exit 1; }

OUT=src-tauri/target/release/upload  # /tmp는 Git Bash 경로라 Windows용 node·curl이 못 찾을 수 있다
rm -rf "$OUT" && mkdir -p "$OUT"
node -e '
const fs = require("fs");
const [version, name, sig, notes, repo, out] = process.argv.slice(1);
const body = fs.readFileSync(notes, "utf8");
fs.writeFileSync(out + "/latest.json", JSON.stringify({
  version, notes: body, pub_date: new Date().toISOString(),
  platforms: { "windows-x86_64": {
    signature: fs.readFileSync(sig, "utf8"),
    url: `https://github.com/${repo}/releases/download/v${version}/${name}`,
  } },
}, null, 2));
fs.writeFileSync(out + "/release.json", JSON.stringify({
  tag_name: `v${version}`, target_commitish: "main", name: `VocaTray ${version} (Windows)`, body,
}));
' "$VERSION" "$NAME" "$EXE.sig" "$NOTES" "$REPO" "$OUT"

TOKEN=$(printf 'protocol=https\nhost=github.com\n\n' | git credential fill | sed -n 's/^password=//p')
[ -n "$TOKEN" ] || { echo "GitHub 로그인 정보를 못 찾음"; exit 1; }
api() { curl -sS -H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json" "$@"; }

ID=$(api -X POST --data-binary @"$OUT/release.json" "https://api.github.com/repos/$REPO/releases" \
  | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const r=JSON.parse(s);if(!r.id){console.error(s);process.exit(1)}console.log(r.id)})')

upload() {
  api -X POST -H "Content-Type: application/octet-stream" --data-binary @"$1" \
    "https://uploads.github.com/repos/$REPO/releases/$ID/assets?name=$2" \
    | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const r=JSON.parse(s);console.log(r.state, r.size, r.browser_download_url)})'
}
upload "$EXE" "$NAME"
upload "$EXE" VocaTray-setup.exe
upload "$OUT/latest.json" latest.json
rm -rf "$OUT"
echo "https://github.com/$REPO/releases/tag/v$VERSION"
