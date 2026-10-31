# Go → Rust 비교 학습: 첫 번째 프로젝트

## 오늘 실행할 실험

```sh
cargo run --locked -- examples/basic.json
```

입력의 `path`를 `/admin/`로 바꾸고 `pathType`을 `Prefix`, `Exact`로 번갈아 실행하세요.
Prefix의 결과는 `/admin`, Exact는 `/admin/`입니다. 어떤 문자열을 바꾸는지보다
두 API의 경로 의미를 보존하는 것이 목표입니다. 예제 복사본에서 실험하면 비교 fixture는 유지됩니다.

다음으로 backend의 `port`를 `{"name":"http"}`로 바꾸세요. Service가 없으면 종료 코드 3입니다.
`services`에 아래 항목을 추가하면 생성된 backendRef의 port는 8080입니다.

```json
{
  "metadata": {"namespace":"apps", "name":"website"},
  "spec": {"ports":[{"name":"http", "port":8080, "targetPort":9000}]}
}
```

## 코드 대응표

Go 파일은 옆 저장소의 `internal/translate/ingress.go`입니다.

| Go 함수/구조 | Rust 위치 | 비교할 점 |
| --- | --- | --- |
| Kubernetes Ingress 타입 | `src/model.rs`의 `Ingress` | 필요한 필드만 모델링, JSON 이름과 Rust 이름 분리 |
| `ClassName` | `src/lib.rs`의 `class_name` | 포인터/nil과 `Option`, 복사 대신 빌린 `&str` 반환 |
| `Convert` | `convert` | `(routes, error)`와 `Result<Vec<Value>, Error>` |
| `pathMatch` | `path_match` | switch와 match, 오류 전파 `?` |
| `backendRef` | `backend_ref` | nil 확인과 `Option::ok_or_else`, 의존성 오류 분리 |
| `RouteName` | `route_name` | byte 길이와 UTF-8 경계, SHA-256 이름 안정성 |
| map + `sort.Strings` | `BTreeMap` | 키 순서가 안정적인 컬렉션 선택 |
| `Rejected`, 일반 error | `Error` enum | 호출자가 오류 종류를 빠짐없이 분기 |

## 1. 소유권과 빌림

```rust
pub fn class_name(ingress: &Ingress) -> Result<&str>
```

함수는 Ingress를 `&Ingress`로 빌려 읽습니다. 반환 문자열도 입력 안의 문자열을 빌립니다.
별도 String 할당이 없고, 반환 참조가 입력보다 오래 살아서는 안 됩니다.
이 함수의 입력 참조가 하나이므로 Rust가 반환 참조의 수명을 그 입력과 연결합니다.

`convert(&Input)`도 입력을 빌립니다. 출력은 새로 만든 `Vec<Value>`가 소유합니다.
호출자는 변환 후에도 입력을 다시 사용할 수 있고, 출력은 입력을 삭제한 뒤에도 사용할 수 있습니다.
`seen`에 넣는 backend와 출력에 넣는 backend가 각각 소유권을 가져야 하는 지점에는 `clone()`이 있습니다.
모든 곳에 clone을 붙이기보다, 왜 독립적인 소유가 필요한지 먼저 확인하세요.

## 2. Option과 Result

Go의 `*string`은 nil 또는 문자열 포인터이고, Rust의 `Option<String>`은 `None` 또는 `Some(String)`입니다.
`as_deref()`는 `Option<String>`을 복사 없이 `Option<&str>`로 읽게 해 줍니다.

`path_match(path)?`는 성공 값을 꺼내고, 오류면 현재 함수에서 바로 반환합니다.
Go의 `if err != nil { return ..., err }`와 대응합니다.
입력 오류를 `unwrap()`으로 처리하면 정상적으로 거부해야 할 입력에서 프로그램이 panic할 수 있으므로
실행 코드에서는 `Result`로 전파합니다. 테스트의 unwrap은 실패를 테스트 실패로 드러내는 용도입니다.

공식 참고: [Rust Result](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html),
[Serde derive](https://serde.rs/derive.html).

## 3. 영구 거부와 재시도

지원하지 않는 wildcard host는 `Rejected`입니다. Service가 아직 보이지 않는 named port는 `Dependency`입니다.
향후 controller는 전자는 사용자 수정이 필요한 상태로 기록하고, 후자는 Service 이벤트나 재시도에서 다시 계산할 수 있습니다.
오류 문자열만 검사하기보다 enum variant를 분기하는 편이 정책을 명확하게 유지합니다.

## 4. 결정적인 출력

Go는 map에서 host를 수집한 다음 정렬합니다. Rust는 `BTreeMap` 순서대로 host를 순회합니다.
각 host 안의 rule 순서는 입력 순서를 유지합니다. 모든 배열을 무조건 정렬하면 기존 동작을 바꿀 수 있습니다.
같은 입력에서 이름과 출력 순서가 안정적이면 controller가 불필요한 업데이트를 줄이는 데 도움이 됩니다.

## 5. 검증과 이력서

공통 fixture는 Rust 테스트와 Go 비교에 모두 사용됩니다. Rust 테스트는 오류 종류와 주요 의미를 검사하고,
Go 비교는 전체 출력과 정확한 오류 메시지를 비교합니다. 두 구현이 같은 실수를 할 수도 있으므로
known expected 값(예: targetPort 9000이 아니라 port 8080)도 별도로 단언합니다.

현재 단계에서 사용할 수 있는 문장:

> Go 기반 Ingress→HTTPRoute 변환 코어를 Rust CLI로 이식하고, 86개 공통 입력 사례에서
> 생성 객체 및 오류 분류·메시지의 호환성을 차등 테스트로 검증.

controller 구현·배포·성능 개선은 각각 실제로 완료하고 측정한 뒤 이력서에 추가하세요.

## 작은 연습

- `Prefix /admin/`과 `Prefix /admin`을 함께 넣으면 rule이 몇 개 남는지 예측하고 확인하기
- 같은 경로의 backend만 바꾸면 오류가 나는 이유 설명하기
- `class_name`을 `String` 반환으로 바꾸었을 때 필요한 코드와 할당 차이 확인하기
- Service snapshot에 UDP와 TCP가 같은 port로 존재하는 경우를 설명하기
- 새로운 경계 사례를 `tests/cases.json`에 넣고 두 구현을 비교하기
