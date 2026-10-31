# Rust controller operations

## 실행과 배포 조건

```sh
cargo build --locked --bin bridge-controller
BRIDGE_CONFIG=bridge.yaml BRIDGE_ONCE=true ./target/debug/bridge-controller
BRIDGE_CONFIG=bridge.yaml ./target/debug/bridge-controller
BRIDGE_CONFIG=bridge.yaml BRIDGE_DRY_RUN=false BRIDGE_SINGLE_WRITER=true ./target/debug/bridge-controller
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
- 2026-10-31 최초 운영 전환 완료: 소스 `d38bc8b87abf79ea2f264c42970732318b7e9ae3`, 이미지 digest `sha256:6955b788b63f81fc63957b39a3d08056eb31d1e119e1b676a39226176f874543`.
- [GitHub Actions 검증·이미지 빌드](https://github.com/colaH16/ingress-gateway-bridge-rs/actions/runs/41160897013)가 성공했고 공개 이미지 pull을 확인했습니다.
- 운영 ServiceAccount를 사용하는 이미지 dry-run Job도 변경 0건/오류 0건으로 완료했습니다.
- Fleet에서 Rust 단일 replica로 교체 후 1/1 Ready, 재시작 0회와 오류 없는 재조정을 확인했습니다.
- 배포된 Rust Pod가 임시 Ingress의 Route 생성·갱신을 실제 수행하고 HTTPS 경로 변경이 반영되는 것을 확인했습니다. 삭제 후 GC와 테스트 리소스 정리도 확인했습니다.
- 기존 HTTPRoute 22개, Gateway 3개, Ingress 66개의 UID/spec을 보존했고, 15개 읽기 전용 HTTP 응답 코드가 전환 전과 일치했습니다.
- 인증 서비스의 비인증 리다이렉트 응답은 로그인 후의 애플리케이션 동작 검증을 의미하지 않습니다. 이 기록은 전환 시점 검증이며 장기간 안정성이나 연속 무중단 측정 결과는 아닙니다.

## 전환과 복구

검증한 이미지 digest와 환경변수를 GitOps Deployment에 지정합니다. `command`/`args`는 지정하지 않습니다.
기존 한 replica/Recreate 전략, ServiceAccount, 제한된 writer RBAC를 유지합니다.
교체 전후 Route UID/spec을 비교하고, 실제 서비스 health와 Route Accepted/ResolvedRefs의 observedGeneration을 확인합니다.
실패하면 GitOps에서 Go 이미지 digest와 Go 인자로 되돌립니다. Route owner와 이름이 같아 Go가 이어서 관리합니다.
CLI나 controller를 운영 writer와 동시에 쓰기 모드로 실행하지 마세요.

## 환경변수로 설정하기

이미지는 `ENTRYPOINT ["/bridge-controller"]`만 지정합니다. 운영 Deployment에는
`command`와 `args`를 지정하지 않습니다. 각 환경변수를 독립적으로 덮어쓸 수 있고,
지정하지 않은 값은 이미지와 프로그램의 기본값을 사용합니다.

| 환경변수 | 이미지 기본값 | 역할 |
| --- | --- | --- |
| `BRIDGE_CONFIG` | `/etc/bridge/bridge.yaml` | ConfigMap 설정 파일 경로 |
| `BRIDGE_DRY_RUN` | `true` | true면 API 쓰기 없이 계획 출력 |
| `BRIDGE_SINGLE_WRITER` | `false` | 쓰기 모드의 단일 writer 운영 확인 |
| `BRIDGE_HEALTH_PROBE_BIND_ADDRESS` | `0.0.0.0:8081` | health/readiness listen 주소 |
| `BRIDGE_ONCE` | `false` | true면 읽기 전용 1회 실행 후 종료 |
| `RUST_LOG` | 프로그램 기본 `info` | 로그 필터 |

```yaml
env:
  - name: BRIDGE_CONFIG
    value: /etc/bridge/bridge.yaml
  - name: BRIDGE_DRY_RUN
    value: "false"
  - name: BRIDGE_SINGLE_WRITER
    value: "true"
  - name: BRIDGE_HEALTH_PROBE_BIND_ADDRESS
    value: "0.0.0.0:8081"
```

CLI는 로컬 디버깅과 호환용으로 남겨두었습니다. 우선순위는 명시한 CLI 옵션 > 환경변수 > 프로그램 기본값입니다.
CLI 하나를 지정해도 나머지 환경변수는 유지됩니다. 바이너리를 직접 실행할 때의 기본 config 경로는 `bridge.yaml`입니다.
잘못된 boolean, 빈 config 경로, single-writer 확인 없는 쓰기, once와 쓰기 동시 사용은 시작 시 거부합니다.
`BRIDGE_SINGLE_WRITER=true`는 잠금이 아니며 한 replica/Recreate와 운영 절차로 단일 writer를 보장합니다.

[환경변수 기반 Deployment 예제](../examples/deployment.yaml)는 dry-run 기본값이며,
기존 ServiceAccount/RBAC와 설정 ConfigMap이 필요합니다. 이미지 tag/digest는 실행 전에 지정하세요.

## Watch 연결 제한

초기 운영 로그에서 45초 HTTP read timeout이 정상 watch를 끊는 현상을 발견했습니다.
수정은 watch의 기본 서버 timeout(290초)보다 긴 330초 read timeout을 사용하고,
일반 snapshot/apply 작업에는 별도 45초 deadline을 유지합니다.
수정 코드의 읽기 전용 100초 관찰에서 경고 없이 여러 회차가 완료됐습니다.

## 현재 운영 이미지와 환경변수 배포

2026-10-31 소스 `7e6c255bb468fc95e829745dfa226782a312cae1`, 이미지
`ghcr.io/colah16/ingress-gateway-bridge-rs@sha256:366d34531d56a3e75f80e8de7f21e03c9b1cf28ba19c864378ff9d962a335e9f`
로 운영 Deployment를 갱신했습니다.
[CI 실행](https://github.com/colaH16/ingress-gateway-bridge-rs/actions/runs/41162026526)에서
Rust 전체 검사와 Go 86개 비교를 통과했습니다. 환경변수 전용 설정·개별 override 보존·CLI 우선순위·잘못된 값 거부에 대한 7개 테스트도 포함됩니다.

운영 Deployment에는 `command`와 `args`가 없고 `BRIDGE_CONFIG`, `BRIDGE_DRY_RUN`,
`BRIDGE_SINGLE_WRITER`, `BRIDGE_HEALTH_PROBE_BIND_ADDRESS`만 env로 지정합니다.
이미지 기본값과 env만 사용한 preflight Job이 변경 0건/오류 0건으로 완료됐고,
배포된 Pod의 임시 Route 생성·갱신·HTTPS 응답과 삭제 후 GC를 다시 검증했습니다.
기존 Route 22개·Gateway 3개·Ingress 66개의 UID/spec과 HTTP 15개 응답 코드를 보존했습니다.
