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
| `Convert` | `convert`, `translate` | `(routes, error)`와 `Result<Vec<HttpRoute<'_>>, Error>` |
| `gatewayv1.HTTPRoute` | `src/route.rs`의 `HttpRoute<'a>` | 전체 API 타입 대신 생성 필드만, 입력 문자열은 빌림 |
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

`convert(&Input)`도 입력을 빌립니다. 출력 `Vec<HttpRoute<'_>>`는 입력의 문자열(namespace, 이름, host, 경로…)을 빌리고,
새로 계산한 route 이름과 `source` annotation만 `String`으로 소유합니다. 그래서 출력은 입력보다 오래 살 수 없습니다.
입력을 버리기 전에 직렬화하거나 `serde_json::to_value`로 독립 사본을 만들어야 하며, 이를 어기면 컴파일러가 거부합니다.
변경 전 구현은 `Vec<Value>`를 소유했고 `seen`과 출력에 같은 backend를 각각 clone했습니다.
지금은 `seen`에 `(&str, Option<i32>)`만 둡니다. 모든 곳에 clone을 붙이기보다, 왜 독립적인 소유가 필요한지 먼저 확인하세요.

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

## 5. typed 구조체, 직렬화, 할당: Go와 무엇이 같고 다른가

Go `translate.Convert`는 처음부터 typed `gatewayv1.HTTPRoute`를 반환했습니다. 초기 Rust 포트는 `json!`으로 `serde_json::Value`를 만들었습니다.
`Value`의 객체는 `BTreeMap<String, Value>`이므로 키마다 `String`, 값마다 노드를 할당합니다. 더 큰 비용은 `json!({"backendRefs":[backend]})`처럼
**이미 만든 `Value`를 다시 넣을 때**였습니다. 매크로가 `Serialize`로 트리를 통째로 다시 만들기 때문에, 중첩 단계마다 같은 내용을 복사했습니다.
측정 결과 convert1 한 번에 할당 180회·약 19KB였고, CPU 샘플의 약 32%가 이 재직렬화였습니다([성능 기록](performance.md)).

typed 구조체로 바꾼 뒤 할당은 12회·약 2.2KB입니다. Go(34회·약 2.8KB)보다 적은 이유는 다음과 같습니다.

- **문자열 빌림:** Go string은 불변이라 대입해도 내용 대신 header(포인터·길이)만 복사합니다. Rust에서 `String`을 대입하려면 clone(새 할당)이 필요하고, 같은 효과를 내려면 `&'a str`로 빌리면 됩니다. 대신 Go에서는 GC가 지켜 주던 "입력이 출력보다 오래 산다"는 조건을 Rust에서는 수명 `'a`로 직접 표현합니다.
- **고정 문자열:** `"HTTPRoute"`, `"Service"` 같은 값은 `&'static str`이라 할당이 없습니다. `ownerReferences`처럼 항상 1개인 목록은 `[OwnerReference; 1]` 배열로 둡니다.
- **필드 순서와 출력 바이트:** serde는 구조체 필드를 선언 순서로 직렬화합니다. 기존 `Value` 출력이 키를 정렬했으므로 필드를 JSON 이름의 알파벳 순서로 선언해 CLI 출력 바이트가 바뀌지 않게 했고, 테스트(`typed_output_serializes_like_sorted_json_value`)가 이를 확인합니다. Go `encoding/json`도 구조체 선언 순서를 쓰지만 Go 출력 순서는 Gateway API 타입의 선언 순서입니다.
- **선택 필드:** Go는 `omitempty` 태그, Rust는 `#[serde(skip_serializing_if = "Vec::is_empty")]`처럼 명시합니다. 빠뜨리면 `"filters": []` 같은 필드가 생겨 기존 객체와 비교할 때 불필요한 업데이트가 생길 수 있으므로 테스트로 고정했습니다.

직렬화 자체의 비용은 변하지 않았습니다(만들어진 결과를 JSON bytes로 만드는 데 Value·typed 모두 약 0.6μs).
그리고 controller는 kube client에 `Value`를 넘기므로 typed→`Value` 변환(약 2.1μs/route)이 생성·비교 단계로 옮겨졌습니다.
"빠른 구조체로 바꾸면 끝"이 아니라 그 비용이 어디로 갔는지까지 측정해야 공정한 비교입니다.

GC 관점: Go는 할당한 메모리를 GC가 나중에 회수하고, 그 비용(동시 mark와 짧은 STW)은 측정 구간 전체에 분산됩니다. Rust는 결과를 drop하는 순간 즉시 해제하며 그 비용이 루프 안에 포함됩니다.
그래서 "GC가 없으니 빠르다"는 성립하지 않았고(변경 전 Rust는 약 3배 느렸음), 실제 차이를 만든 것은 할당 횟수와 복사량이었습니다.

## 6. 검증과 이력서

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
- `HttpRoute`의 필드 하나를 알파벳 순서에서 벗어나게 옮기고 어떤 테스트가 실패하는지 확인하기
- `convert` 결과를 입력보다 오래 보관하는 코드를 써 보고 컴파일 오류 메시지 읽기
- `python3 docs/benchmarks/run_diag.py`로 할당 수를 재고, `BackendRef.name`을 `String`으로 바꿨을 때 늘어나는 할당 예측하기
- Service snapshot에 UDP와 TCP가 같은 port로 존재하는 경우를 설명하기
- 새로운 경계 사례를 `tests/cases.json`에 넣고 두 구현을 비교하기
