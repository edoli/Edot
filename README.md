# Edot

Windows용 순수 텍스트 노트 앱입니다. Rust와 GPUI로 구현하며 트리 사이드바, 여러 탭, 자동 저장, 현재·전체 노트 검색과 바꾸기를 제공합니다.

## 빌드와 실행

Windows에서 rustup, C++ 빌드 도구·Windows SDK, Python 3.11 이상이 필요합니다.

```powershell
python tools/configure_path_remap.py
cargo build --release --target-dir target
.\target\release\edot.exe
```

첫 명령은 현재 PC에 맞는 소스 경로 치환 설정을 생성합니다. checkout·Rust/Cargo 캐시 위치 또는 Git 의존성 revision을 바꾼 뒤에는 다시 실행합니다.

새 Space의 기본 위치는 `%LOCALAPPDATA%\Edot\spaces\Default`입니다. `--space <폴더>` 또는 `EDOT_SPACE_DIR`로 지정하거나 사이드바 하단의 Space 목록에서 추가·전환할 수 있으며, 다음 실행에는 마지막 Space를 엽니다. Space 이름은 각 폴더의 `space.ini`에 저장합니다. 목록의 삭제는 등록만 제거하고 폴더와 노트를 보존합니다. 본문은 `notes/`의 UTF-8 텍스트, 일반 텍스트 제목·트리는 `tree.tsv`(`EDOT4`), 탭·트리 펼침 상태는 `session.bin`에 24바이트 헤더와 8바이트 ID 배열로 저장합니다. 기존 `session.tsv`는 바이너리가 없을 때만 읽고 다음 정상 저장 때 전환하며 원본을 보존합니다. 목록의 ID·부모 ID는 패딩 없는 11자 base64url이며 본문 파일명은 10진수를 유지하고 세션 ID는 little-endian 바이너리로 저장합니다. 목록은 `EDOT4`만 읽으며 이전 목록 형식의 읽기·자동 변환은 지원하지 않습니다. 삭제한 본문은 Space의 `trash/`에 보관합니다. 저장 파일·경로·실행 옵션도 Space 명칭을 사용하며 이전 명칭의 읽기와 자동 변환은 지원하지 않습니다.

전역 설정은 `%APPDATA%\Edot\settings.ini`에 저장하며 `EDOT_CONFIG_DIR`로 설정 폴더를 바꿀 수 있습니다. 이전 설정 형식의 읽기와 자동 변환은 지원하지 않습니다.

## 개발 문서

- [IMPLEMENTATION.md](IMPLEMENTATION.md): 현재 화면·동작·저장 형식·단축키·코드 구조와 검증 명령
- [GPUI_FORK.md](GPUI_FORK.md): GPUI fork의 기능 제한과 upstream 업데이트 방법
- [AGENTS.md](AGENTS.md): 개발 작업 지침

## 배포

GitHub Actions의 `.github/workflows/ci.yml`은 main push, main 대상 PR, 수동 실행과 `v*` 태그 push에서 동작합니다. CI가 Cargo 버전의 숫자 세 부분(각 0~65535, 선행 0 없음)을 검사하고 태그가 있으면 정확히 `v<Cargo 버전>`인지 확인합니다. 버전 검사 → stable Rust의 `release-dist` 빌드 → OS별 패키징을 수행하며 태그 실행만 GitHub Release를 공개합니다.

Windows x64, macOS Apple Silicon, Linux x64를 배포합니다. Windows CRT는 정적으로 링크하고 Linux 앱 라이브러리는 정적으로 묶되 GUI·GPU·libc 등 시스템 라이브러리는 사용합니다. macOS 앱은 인증서 없이 ad-hoc 서명하며 Apple 공증은 수행하지 않습니다.


앱 업데이트는 GitHub Releases API에서 대상 파일과 SHA-256·크기를 읽어 검증합니다. 설치형은 기존 설치 경로·범위를 유지하고 포터블은 ZIP의 파일을 교체한 뒤 다시 실행합니다. 검증 정보가 없거나 다운로드 검증에 실패하면 업데이트하지 않습니다.

릴리스 준비는 Cargo.toml 버전 변경 → Cargo.lock 갱신 → 검증 → 두 파일을 포함한 커밋 → 같은 버전의 태그 생성 → main과 태그 push 순서로 수행합니다. 별도 버전 변경·커밋·태그 자동화는 없습니다.

Release Assets는 OS별 ZIP 3개와 EXE·DMG·DEB·AppImage 4개만 공개합니다. 별도 업데이트 정보 파일이나 공개 체크섬 파일은 배포하지 않으며, 앱은 GitHub Asset의 SHA-256으로 검증합니다. Windows 설치형 판별은 HKCU/HKLM의 InstallLocation과 현재 실행 경로를 대조합니다. Linux DEB는 cargo-deb로 생성하고 실행 파일을 /usr/bin에 설치합니다. README·LICENSE는 설치 payload에서 제외합니다.
