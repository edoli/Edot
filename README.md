# Edot

Windows용 순수 텍스트 메모 앱입니다. Rust와 GPUI로 구현하며 트리 사이드바, 여러 탭, 자동 저장, 현재·전체 메모 검색과 바꾸기를 제공합니다.

## 빌드와 실행

Windows에서 Rust MSVC 툴체인, C++ 빌드 도구·Windows SDK, Python 3.11 이상이 필요합니다. 최초 빌드는 고정된 GPUI Git 소스와 Cargo 의존성을 다운로드합니다.

```powershell
python tools/configure_path_remap.py
cargo build --release --target-dir target
.\target\release\edot.exe
```

첫 명령은 현재 PC에 맞는 소스 경로 치환 설정을 생성합니다. checkout·Rust/Cargo 캐시 위치 또는 Git 의존성 revision을 바꾼 뒤에는 다시 실행합니다.

저장소 기본 위치는 `%LOCALAPPDATA%\Edot`이며 `EDOT_DATA_DIR`로 바꿀 수 있습니다. 본문은 `notes/`의 UTF-8 텍스트, 제목·트리는 `tree.tsv`, 열린 탭은 `session.tsv`, 설정은 `settings.tsv`에 저장합니다. 삭제한 본문은 저장소의 `trash/`에 보관합니다.

## 개발 문서

- [IMPLEMENTATION.md](IMPLEMENTATION.md): 현재 화면·동작·저장 형식·단축키·코드 구조와 검증 명령
- [GPUI_FORK.md](GPUI_FORK.md): GPUI fork의 기능 제한과 upstream 업데이트 방법
- [AGENTS.md](AGENTS.md): 개발 작업 지침
