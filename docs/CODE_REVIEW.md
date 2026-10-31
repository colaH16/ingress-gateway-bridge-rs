# Claude 독립 코드 검토와 조치 (2026-10-31)

사용자 요청으로 Claude Code (`claude-opus-5-5`)에 공개 소스를 읽기 전용으로 전달했습니다. 1차는 controller/runtime/설정, 추가 검토는 Rust lib.rs/model.rs/options.rs와 Go translate/ingress.go/annotations.go, controller/config를 직접 대조했습니다. 초기 검토 1회는 최종 보고서 전에 종료되어 재시도했고 완료된 보고서를 기준으로 조치했습니다. Claude는 실행 검증을 하지 않았습니다.

반영한 항목:

- SIGTERM/SIGINT receiver를 루프 밖에 유지하고 snapshot/apply 중에도 종료 감지.
- 개별 Ingress 실패와 전역 snapshot 실패를 분리하여 다른 watch 이벤트가 30초 전역 backoff에 묶이지 않도록 수정.
- API 지연이 정상 프로세스 재시작을 유발하지 않도록 liveness와 스냅샷 freshness readiness 분리.
- namespace label selector의 키/값과 In/NotIn/Exists/DoesNotExist values 조건을 검증하여 잘못된 selector를 거부.

검증: 기존 변환/계획/CLI/환경변수 테스트 및 selector 회귀 테스트, 로컬 fake Kubernetes API에서 지속적인 Ingress 오류 중 watch 처리와 응답이 멈춘 snapshot 중 SIGTERM 종료를 확인했습니다. 테스트 API 외에 클러스터에 쓰지 않습니다. `python3 tests/runtime.py`로 재현합니다.

Claude의 추가 변환 코드 대조에서 경로 검증, backend 해석, 중복 규칙, 해시 기반 이름, 헤더 annotation, redirect, owner reference/UID/RV 보호에 구체적인 의미 회귀는 보고되지 않았습니다. 이는 전체 동등성 증명이 아닙니다. 기존 86개 Go oracle 사례도 유지합니다.

남은 설계 차이: informer 캐시 대신 watch 알림 후 전체 snapshot, 소스별 rate limiter 부재, Warning Event 대신 로그, 일부 null 입력의 엄격한 거부. 클러스터 규모와 잦은 변경에 따른 비용을 계속 측정해야 합니다. 프로파일링·typed 출력 최적화는 KubeShelf 운영 전환 이후 별도 작업으로 진행하며, 현재 성능 기록은 변경 전 구현의 값을 보존합니다.

## 수정본 운영 적용

2026-10-31 코드 `7940558838456bb4273e34999be179b53e584d16`를 `ghcr.io/colah16/ingress-gateway-bridge-rs@sha256:df0be5f9ab77b62df582c2192887f5ce5a237ea247659ceccc87a683a3bf5424`로 배포했습니다. CI 성공, Rust 테스트 18개 및 fake API 종료/이벤트 격리 회귀 검증 통과. 운영 1/1 Pod ready·재시작 0, healthz/readyz HTTP 200. 관리 중인 HTTPRoute 22개의 UID와 spec이 수정 배포 전후 동일했습니다.
