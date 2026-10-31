# ingress-gateway-bridge-rs

Rust repository: https://github.com/colaH16/ingress-gateway-bridge-rs

Go `ingress-gateway-bridge`의 변환 코어를 Rust로 옮기는 비교 학습 프로젝트입니다.
오프라인 변환 CLI와 **단일 writer Kubernetes 컨트롤러**를 제공합니다.

운영 Go 기준: `41b4b395f37abd0b03ba858f1da6c32c02834813`.
컨트롤러 실행과 제약은 [운영 가이드](docs/operations.md)를 참고하세요.

**운영 상태:** 2026-10-31부터 개인 Kubernetes 클러스터에서 Rust controller로 HTTPRoute 22개를 관리합니다. [운영 검증 기록](docs/operations.md)을 참고하세요.

학습 순서: **ingress-gateway-bridge → gotalk → KubeShelf**.

## 실행

Rust stable(Cargo 포함)을 설치한 환경에서:

```sh
cargo run --locked -- examples/basic.json
cargo run --locked -- - < examples/basic.json
cargo run --locked -- --help
cargo test --locked
```

입력은 `ingress`, `binding`, 선택적인 `services`, `controllerName`을 갖는 JSON입니다.
예제의 `metadata.uid`는 학습용 값이며 실제 API 객체는 Kubernetes가 발급한 UID를 사용합니다.
`binding.parentRefs`는 기존 Gateway를 명시합니다. 이름으로 지정한 backend port를 해석하려면
같은 namespace의 Service 스냅샷을 `services`에 넣습니다. `targetPort` 대신 Service의 `port`를 사용합니다.

성공 출력은 `{"routes": [...]}`, 변환 오류는 `{"error":{"kind":"rejected|dependency","message":"..."}}`입니다.
stdout은 JSON 결과, stderr는 입력·설정·I/O 오류에 사용합니다.

| 종료 코드 | 의미 |
| --- | --- |
| 0 | 변환 성공 |
| 1 | JSON, 설정, 인자 또는 I/O 오류 |
| 2 | 지원하지 않는 입력; 원본이나 정책 변경 필요 |
| 3 | named port 해석에 필요한 Service 정보 부족; 최신 스냅샷으로 재시도 가능 |

## 범위

- Prefix/Exact 경로 변환, host별 HTTPRoute 분리와 정렬
- 동일 host/path 중복 제거와 충돌 거부
- 숫자/named Service port, TCP 검증
- TLS External 정책, 미지원 필드·bridge annotation 거부
- Go와 동일한 SHA-256 기반 이름 및 owner reference
- legacy class 충돌 검사

이 CLI의 binding은 **호출자가 이미 선택한 값**입니다. class가 없어도 순수 변환은 수행합니다.
실제 controller의 class 선택·IngressClass 소유권 확인·watch·reconcile·API 쓰기는 별도 `bridge-controller` 바이너리가 담당합니다.
Kubernetes API 객체를 입력한다는 전제이며 Kubernetes 전체 스키마 validator는 아닙니다.
JSON의 자료형이 잘못되거나, 배열/맵에 명시적 null을 넣는 등의 디코딩 동작은 Go와 완전 호환을 보장하지 않습니다.
현재는 JSON 입력만 지원합니다. controller 설정 YAML 로딩·검증은 `bridge-controller`에서 지원합니다.
검증 결과와 운영 적용 상태는 운영 가이드에 기록합니다.

## Go와 비교

Go 1.25 이상, Rust, Python 3와 의존성 다운로드 접근이 필요합니다.
기본값은 옆 `worktrees/ingress-gateway-bridge/independent-review-20261019` 체크아웃입니다. 다른 환경에서는 `--go-source`를 지정하세요.

```sh
python3 tools/compare.py
# 다른 경로를 사용할 때
python3 tools/compare.py --go-source /path/to/ingress-gateway-bridge
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

비교 도구는 임시 Go 모듈에서 원본의 `ClassName`과 `Convert`를 직접 호출합니다.
원본 체크아웃에 파일을 추가하지 않습니다. `tests/cases.json`의 86개 사례에 대해
성공 결과 전체, 오류 분류·메시지, Rust 종료 코드를 비교합니다.
Go가 출력하는 빈 `status`와 null `creationTimestamp`만 정규화하며 라우팅 필드는 그대로 비교합니다.
이 검증은 86개 사례의 일치를 뜻하며 모든 입력에 대한 동등성 증명은 아닙니다.

기준 Go 커밋: `41b4b395f37abd0b03ba858f1da6c32c02834813`.
원본을 수정한 뒤 비교하면 현재 체크아웃을 기준으로 검사하고 실제 HEAD를 표시합니다.

## 읽는 순서

1. `examples/basic.json`: 입력과 실행 결과를 먼저 확인
2. [Go ↔ Rust 학습 가이드](docs/learning.md): 원본 함수와 Rust 대응 관계
3. `src/model.rs`: 입력 모델과 `Option`, `Vec`, Serde
4. `src/lib.rs`: 빌려 읽는 입력, 소유하는 출력, 오류 전파
5. `tests/translation.rs`와 `tests/cases.json`: 동작 계약
6. `src/main.rs`: 파일·stdin·종료 코드 처리

Rust 출력을 `serde_json::Value`로 표현한 것은 첫 단계의 의존성과 비교 범위를 작게 유지하기 위한 선택입니다.
입력은 명시적인 구조체로 해석합니다. 컨트롤러 단계에서는 Kubernetes/Gateway 타입을 사용해
출력 구조를 컴파일 시점에도 검증하는 방향으로 확장합니다.

## 학습 진행

변환 CLI → controller 계획 함수 → 실제 API 실행 → 운영 검증 순서로 읽습니다.
`src/controller.rs`의 `plan`은 API에 쓰지 않는 순수 계획 함수이고,
`runtime`은 감시·스냅샷 조회·조건부 쓰기·재시도를 담당합니다.
Go와 Rust를 비교한 다음 gotalk, 마지막에 KubeShelf로 진행합니다.

## 출처

Go 원본: https://github.com/colaH16/ingress-gateway-bridge

원본의 Apache-2.0 라이선스를 보존합니다. 원본 번역 알고리즘을 바탕으로 작성한 Rust 학습 포트이며,
변환 의미는 Go 원본에서 이어받았으며, 실제 검증 범위와 운영 기록은 운영 가이드에 명시합니다.
