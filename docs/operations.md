# Rust controller operations

## 실행과 배포 조건

```sh
cargo build --locked --bin bridge-controller
./target/debug/bridge-controller --config=bridge.yaml --once
./target/debug/bridge-controller --config=bridge.yaml --dry-run=true
./target/debug/bridge-controller --config=bridge.yaml --dry-run=false --single-writer=true
```

`--once`는 읽기 전용입니다. 기본 dry-run은 어떤 API 쓰기도 하지 않습니다.
쓰기 모드는 **한 replica, Recreate 전략, 같은 class를 쓰는 다른 controller가 없는 상태**에서 사용합니다.
leader election은 구현하지 않았고 true 요청은 시작 시 거부합니다. 자동으로 다른 writer를 감지하는 분산 잠금은 없습니다.
설정 파일은 시작 시 읽으므로 변경 시 재시작하세요. 인증은 kubeconfig 또는 Pod ServiceAccount를 사용합니다.
Secret, Gateway, Ingress, IngressClass에 쓰기 권한이 필요하지 않습니다.
현재 배포 이미지의 지원 플랫폼은 linux/amd64입니다.

## 처리 방식

Ingress, IngressClass, Service, HTTPRoute, Gateway, Namespace의 watch가 전체 재조정을 깨웁니다.
1초 이상 합쳐 처리하며 30초 주기 재조정으로 누락 이벤트와 의존성 오류를 다시 확인합니다.
오류 시 2~30초까지 지연을 늘립니다. 단일 직렬 writer이며 소규모 개인 클러스터를 대상으로 합니다.
대규모 클러스터용 indexed queue나 여러 replica는 현재 범위에 포함하지 않습니다.

매 회차 paginated list가 모두 성공한 뒤 계획을 계산합니다. 조회 실패를 리소스 삭제로 해석하지 않습니다.
이름 충돌과 Gateway 허용 범위를 모든 desired Route에 대해 검사한 뒤 쓰기를 시작합니다.
원본 UID/resourceVersion을 쓰기 직전에 재확인하고, update에는 기존 resourceVersion,
delete에는 UID와 resourceVersion precondition을 넣습니다. 여러 Route의 변경은 원자적 트랜잭션이 아니며 실패 시 다음 회차에서 수렴합니다.
동시에 바뀐 의존성은 다음 watch/resync에서 반영합니다.

미지원 입력은 해당 Ingress의 소유 Route를 철회합니다. named Service 누락 등 일시적인 의존성 문제는 기존 Route를 보존합니다.
클래스 삭제·변경 시 소유 Route를 정리합니다. Ingress 삭제 시 Kubernetes ownerReference GC를 사용합니다.
외부 metadata와 Route status를 보존하며 spec이 동일하면 update하지 않습니다.
기존 Go와 동일한 controller annotation·Ingress owner UID·Route 이름을 사용하므로 전환 시 Route를 재생성할 필요가 없습니다.

## 관측과 제한

`/healthz`와 `/readyz`가 기본 `:8081`에서 제공됩니다. 첫 API 스냅샷 처리 후 ready가 됩니다.
마지막 회차가 90초 넘게 완료되지 않으면 ready=false, 180초 이상이면 health=false입니다.
개별 source 의존성 오류는 JSON 로그의 `errors`에 표시되며 readiness와 구분됩니다.
정상 회차의 `changes`, `errors`, source/route 수와 개별 작업·오류를 JSON 로그로 남깁니다.
Kubernetes Warning Event 및 Prometheus metrics endpoint는 아직 제공하지 않습니다.

## 검증

- 86개 공통 변환 사례를 운영 Go 커밋과 비교합니다.
- 단위 테스트는 멱등성, 충돌 거부, metadata 보존, 클래스 생명주기, 의존성/거부 구분, namespace selector를 검사합니다.
- CLI 종료 코드 및 malformed input을 별도 검증합니다.
- 운영 설정의 읽기 전용 비교 결과: Ingress 66개, HTTPRoute 22개, 변경 계획 0건, 오류 0건 (2026-10-31).
- 실제 클러스터 별도 테스트 class에서 생성/named port, Prefix/Exact HTTPS 6경로, 멱등성, 재시작 UID 유지, 갱신, 의존성 오류 보존, 미지원 입력 철회, class 삭제/복구, HTTP→HTTPS, response header, stale redirect 정리, Ingress 삭제 GC를 검증했습니다.
- 테스트 정리 후 기존 운영 Route 22개 UID/spec이 모두 동일했습니다.
- 최종 운영 적용 결과는 완료 후 별도로 기록합니다.

## 전환과 복구

검증한 이미지 digest를 GitOps Deployment에 지정하고 `--single-writer=true`를 추가합니다.
기존 한 replica/Recreate 전략, ServiceAccount, 제한된 writer RBAC를 유지합니다.
교체 전후 Route UID/spec을 비교하고, 실제 서비스 health와 Route Accepted/ResolvedRefs의 observedGeneration을 확인합니다.
실패하면 GitOps에서 Go 이미지 digest와 Go 인자로 되돌립니다. Route owner와 이름이 같아 Go가 이어서 관리합니다.
CLI나 controller를 운영 writer와 동시에 쓰기 모드로 실행하지 마세요.
