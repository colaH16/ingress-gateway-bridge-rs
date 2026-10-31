# Claude 독립 코드 검토와 조치 (2026-10-31)

사용자 요청으로 Claude Code (`claude-opus-5-5`)에 공개 소스를 읽기 전용으로 전달했습니다. 1차는 controller/runtime/설정, 추가 검토는 Rust lib.rs/model.rs/options.rs와 Go translate/ingress.go/annotations.go, controller/config를 직접 대조했습니다. 초기 검토 1회는 최종 보고서 전에 종료되어 재시도했고 완료된 보고서를 기준으로 조치했습니다. Claude는 실행 검증을 하지 않았습니다.

반영한 항목:

- SIGTERM/SIGINT receiver를 루프 밖에 유지하고 snapshot/apply 중에도 종료 감지.
- 개별 Ingress 실패와 전역 snapshot 실패를 분리하여 다른 watch 이벤트가 30초 전역 backoff에 묶이지 않도록 수정.
- API 지연이 정상 프로세스 재시작을 유발하지 않도록 liveness와 스냅샷 freshness readiness 분리.
- namespace label selector의 키/값과 In/NotIn/Exists/DoesNotExist values 조건을 검증하여 잘못된 selector를 거부.

검증: 기존 변환/계획/CLI/환경변수 테스트 및 selector 회귀 테스트, 로컬 fake Kubernetes API에서 지속적인 Ingress 오류 중 watch 처리와 응답이 멈춘 snapshot 중 SIGTERM 종료를 확인했습니다. 테스트 API 외에 클러스터에 쓰지 않습니다. `python3 tests/runtime.py`로 재현합니다.

Claude의 추가 변환 코드 대조에서 경로 검증, backend 해석, 중복 규칙, 해시 기반 이름, 헤더 annotation, redirect, owner reference/UID/RV 보호에 구체적인 의미 회귀는 보고되지 않았습니다. 이는 전체 동등성 증명이 아닙니다. 기존 86개 Go oracle 사례도 유지합니다.

남은 설계 차이: informer 캐시 대신 watch 알림 후 전체 snapshot, 소스별 rate limiter 부재, Warning Event 대신 로그, 일부 null 입력의 엄격한 거부. 클러스터 규모와 잦은 변경에 따른 비용을 계속 측정해야 합니다. 프로파일링·typed 출력 최적화는 KubeShelf 운영 전환 이후 별도 작업으로 진행했습니다(아래 절). 변경 전 구현의 성능 기록은 보존합니다.

## 수정본 운영 적용

2026-10-31 코드 `7940558838456bb4273e34999be179b53e584d16`를 `ghcr.io/colah16/ingress-gateway-bridge-rs@sha256:df0be5f9ab77b62df582c2192887f5ce5a237ea247659ceccc87a683a3bf5424`로 배포했습니다. CI 성공, Rust 테스트 18개 및 fake API 종료/이벤트 격리 회귀 검증 통과. 운영 1/1 Pod ready·재시작 0, healthz/readyz HTTP 200. 관리 중인 HTTPRoute 22개의 UID와 spec이 수정 배포 전후 동일했습니다.

## typed 출력 최적화 (2026-10-31, 운영 미배포)

사용자 요청으로 Claude Code (`claude-opus-5-5`)가 이번에는 **구현과 측정을 직접 수행**했습니다. 따라서 이 절의 검토는 구현자 자신의 검토이며 독립 검토가 아닙니다. 공개 전 root(사람)가 diff와 운영 route 비교를 검토합니다.

변경 범위: `src/route.rs`(신규 typed 출력), `src/lib.rs`(`convert`/`translate`, `route_name`), `src/options.rs`, `src/main.rs`(CLI 출력), `src/controller.rs`(`plan`의 역직렬화·비교·clone 시점). 환경변수 설정, 종료 신호, Ingress별 backoff 격리, health/readiness, selector 검증 코드는 수정하지 않았습니다.

실행한 검증:

- `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`(23개, 기존 18개 + 신규 5개)
- `python3 tools/compare.py`: Go `41b4b39` 원본과 86개 공통 사례 전체 일치
- 변경 전 CLI(`3977e61`)와 변경 후 CLI의 stdout/stderr/종료 코드를 86개 사례와 추가 조합 4개(헤더 filter 2종+redirect+여러 host+hostless, 여러 parentRef와 sectionName 유무, named port 중복)에서 **바이트 단위로 비교**해 동일
- `cargo build --locked --release`, `python3 tests/runtime.py`(fake API: 지속적인 Ingress 오류 중 watch 처리, 막힌 snapshot 중 SIGTERM 종료)

신규 테스트가 고정하는 위험: typed 직렬화가 기존 정렬 `Value`와 같은 바이트인지, 선택 필드(`hostnames`, `sectionName`, `filters`, `backendRefs`)가 비었을 때 생략되는지, filter마다 modifier가 하나인지, redirect route의 parent/annotation/owner, 결정적 route 이름 값, controller의 metadata merge(누락 label 추가, 외부 label/annotation 보존, 비객체 필드 교체, 외부 label만 추가된 경우 무변경), 생성 action이 변환 결과의 직렬화와 같은지.

남은 위험과 한계: 출력이 입력 문자열을 빌리므로 수명이 컴파일 시점에 강제됩니다. `plan`은 기존 route와 비교하려고 spec/annotations/labels를 `Value`로 바꾸며, 그 비용은 측정해 공개했습니다. 86개 사례와 추가 조합은 모든 입력의 동등성 증명이 아닙니다. 운영 클러스터 적용과 운영 route 비교는 아직 하지 않았습니다.
