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
