# ingress-gateway-bridge-rs 성능 실험

이 문서는 두 측정을 담습니다. 아래 [최초 측정](#최초-측정-변경-전-구현-보존)은 `serde_json::Value` 출력 구현이 Go보다 약 3배 느렸던 결과이며 수정하지 않고 보존합니다. 먼저 그 원인을 프로파일링하고 typed 출력으로 바꾼 뒤 같은 기계에서 다시 짝지어 측정한 결과입니다.

## typed HTTPRoute 출력 최적화 (2026-10-31, 운영 미배포)

**상태:** 이 변경은 아직 운영 이미지에 배포되지 않았습니다. 운영 중인 controller는 `7940558`(변경 전 변환 구현)입니다. 아래 수치는 공유 VM의 microbenchmark이며 운영 reconcile 지연이나 사용자 트래픽 지연이 아닙니다.

### 원인 측정

변경 전 소스 `3977e61`을 `git archive`로 고정해 같은 입력·toolchain·harness로 기준을 재현한 뒤 원인을 측정했습니다. `perf`/`valgrind`가 PATH에 없어 다음 두 가지를 **별도 진단 바이너리**(`docs/benchmarks/diag`)로 측정했습니다. 진단 바이너리는 headline 시간 측정에 쓰지 않습니다.

1. 할당 계수: global allocator를 원자적 카운터로 감싼 바이너리. 할당 횟수, `realloc` 횟수, 요청 바이트(할당 크기 + realloc 증가분)를 셉니다. 해제 바이트나 RSS는 아닙니다.
2. CPU 샘플: `pprof` crate(SIGPROF 기반, 요청 4999Hz)로 convert1을 반복하며 호출 스택을 샘플링했습니다. 변경 전 2,971개, 변경 후 1,213개 샘플입니다. 실제 샘플률은 요청보다 낮고 inline된 함수는 호출자에 합쳐지므로 백분율은 대략적인 분포입니다.

| 단계 (진단 바이너리, 할당 계수) | 변경 전 할당/바이트 | 변경 후 할당/바이트 |
|---|---:|---:|
| convert1 (+drop) | 180 / 18,993 B | **12 / 2,207 B** |
| convert22 (+drop) | 3,960 / 417,918 B | **264 / 48,590 B** |
| convert1 + JSON bytes (`to_vec`) | 181 / 20,017 B | 13 / 3,231 B |
| `options::parse` (annotation 없음) | 4 / 328 B | 0 / 0 B |
| `route_name` | 9 / 104 B | 1 / 31 B |

참고로 Go의 같은 convert1은 `runtime.MemStats` 기준 34회/2,776 B, convert22는 748회/61,082 B입니다(계수 방식이 다르므로 대략적 비교).

변경 전 CPU 샘플에서 `convert`가 88.6%(inclusive)였고, 그중 **`serde_json::to_value::<&Value>`가 31.8%**였습니다. `json!` 매크로에 이미 만든 `Value`(backendRef, rule, parentRefs, filters)를 넣으면 `Serialize`를 거쳐 트리 전체를 다시 복사합니다. 중첩 단계마다 이 복사가 반복되었습니다. self 시간은 `_int_malloc` 24.3%, `__free`·free chunk 처리 약 16%, `BTreeMap<String, Value>` 삽입 8.5%로 할당·해제와 map 구성이 대부분이었습니다. 원본 표는 [before/profile.txt](benchmarks/results/diag/before/profile.txt), [flamegraph](benchmarks/results/diag/before/flame.svg)입니다.

### 바꾼 것

- `src/route.rs`: 생성하는 필드만 가진 typed `HttpRoute` 구조체를 추가했습니다. 입력에서 그대로 복사하던 문자열(namespace, 이름, UID, host, 경로, Service 이름, controller 이름)은 `&'a str`로 **빌립니다**. 새로 계산하는 route 이름과 `source` annotation만 `String`입니다. 필드를 JSON 이름의 알파벳 순서로 선언해, 기존 BTreeMap 기반 `Value`(키 정렬) 출력과 바이트 단위로 같게 직렬화합니다.
- `convert`/`translate`: 위 구조체를 직접 구성하고, 중복 경로 검사는 `Value` 대신 `(Service 이름, Option<port>)`로 비교합니다. redirect route를 `Value` clone 후 덮어쓰지 않고 필요한 필드만 구성합니다.
- `options::parse`: 접두사가 같은 annotation 키가 BTreeMap에서 연속이라는 점을 이용해 range 한 번으로 찾습니다. 조회 키 4개를 `format!`으로 만들던 할당이 없습니다. 헤더 filter도 typed입니다.
- `route_name`: 해시 byte마다 `format!`하던 것을 정확한 크기의 버퍼 하나로 바꿨습니다.
- controller `plan`: Ingress/Service를 `Value` clone 없이 `&Value`에서 역직렬화하고, 기존 route와 비교할 spec·annotations·labels만 `Value`로 만듭니다. 기존 객체 clone은 실제로 변경이 필요할 때만 합니다. 결과 의미(merge 대상 키, 외부 label/annotation 보존, 비객체 필드 교체)는 회귀 테스트로 고정했습니다.

**비용이 사라진 것이 아니라 옮겨진 부분:** Kubernetes API에 보내는 경로는 여전히 `Value`/JSON입니다. typed 출력을 `Value`로 바꾸는 비용(아래 `to_value`, 약 2.1μs/route)은 이제 controller의 생성·비교 단계에서 냅니다. 그래서 순수 convert 수치만이 아니라 convert+직렬화, controller 생성 경로, controller 정상 상태 계획을 함께 측정했습니다.

같은 프로세스 안에서 단계를 번갈아 30 batch씩 잰 진단 결과(시스템 allocator, 계수 없음, 중앙값):

| 단계 (Ingress 1개) | 변경 전 | 변경 후 |
|---|---:|---:|
| convert (+drop) | 7.45 μs | 0.30 μs |
| 만들어진 결과의 JSON bytes 직렬화 (`to_vec`) | 0.72 μs | 0.55 μs |
| 만들어진 결과를 `Value`로 변환 (`to_value`) | (convert가 이미 Value) | 2.13 μs |
| convert + `to_vec` | 8.53 μs | 0.91 μs |

직렬화 자체는 변경 전후 비슷합니다(Value와 typed 모두 약 0.6μs). 줄어든 것은 `Value` 트리를 여러 번 만들고 복사하던 비용입니다. 원본: [before/stages.jsonl](benchmarks/results/diag/before/stages.jsonl), [after/stages.jsonl](benchmarks/results/diag/after/stages.jsonl). 변경 후 CPU 샘플의 상위는 SHA-256 route 이름(19.6%, 해시 블록 12.9%), 구조체 이동 memcpy, 경로 검증이었습니다([after/profile.txt](benchmarks/results/diag/after/profile.txt)).

### 짝지은 회전 측정

같은 VM, CPU 2 고정, Go `GOMAXPROCS=1`, 로컬 빌드 없음. 한 round마다 모든 작업을 각 변형으로 1회씩 실행하고 변형 순서를 회전했습니다. 7 round 전부를 선택 없이 공개합니다. 범위는 신뢰구간이 아닌 관측 최솟값~최댓값이고 배율 범위는 같은 round끼리 짝지은 비율입니다. 변형:

- Go GC 켬: `GOGC=100`, Go GC 끔: `GOGC=off`(한정 실험, 운영 설정 아님) — Go 원본 `41b4b39`
- Rust 변경 전: `3977e61` + 같은 harness(`--cfg bridge_before`), Rust 변경 후: 현재 작업 트리
- 기존 공개 harness 바이너리(`results/micro.json`을 만든 것)도 class/convert에 함께 돌렸습니다. 변경 전 대비 convert1 1.07배(0.88~1.32), convert22 0.97배(0.85~1.15)로, harness 수정이 측정 대상을 바꾸지 않았음을 확인했습니다.

모든 실행에서 샘플 출력(Go의 빈 `status`/null `creationTimestamp`만 제거)이 변형 간 같은지 실행기가 검사합니다. 결과는 루프 안에서 `black_box`/Go 전역 sink로 소비하고, Rust는 결과 drop도 측정 구간 안에서 합니다.

| 작업 (1 op) | Go GC 켬 | Go GC 끔 | Rust 변경 전 | Rust 변경 후 | 후/전 | 후/Go GC 켬 |
|---|---:|---:|---:|---:|---:|---:|
| class 조회 | 2.15 ns (1.94~5.42) | 2.55 ns (1.86~6.61) | 10.87 ns (7.39~11.54) | 8.45 ns (7.01~11.85) | 0.73 (0.61~1.34) | 3.64 (1.35~6.10) |
| convert1 | 2.145 μs (1.780~3.046) | 2.009 μs (1.670~2.556) | 6.593 μs (6.094~7.481) | **0.347 μs** (0.252~0.830) | 0.049 (0.038~0.136) | 0.16 (0.08~0.47) |
| convert22 | 44.56 μs (39.19~50.53) | 45.28 μs (35.56~53.40) | 145.36 μs (119.84~159.59) | **19.12 μs** (6.29~20.78) | 0.13 (0.05~0.15) | 0.40 (0.15~0.45) |
| convert1 + JSON bytes | 6.405 μs (4.085~9.528) | 5.090 μs (3.350~6.423) | 7.988 μs (6.454~9.845) | **2.693 μs** (0.845~2.759) | 0.31 (0.09~0.42) | 0.28 (0.16~0.45) |
| convert22 + JSON bytes | 90.86 μs (82.72~115.55) | 85.27 μs (78.98~94.57) | 167.88 μs (148.03~178.43) | **35.93 μs** (19.57~58.55) | 0.20 (0.13~0.33) | 0.31 (0.20~0.71) |
| controller 생성 경로 1개 (Rust) | — | — | 7.864 μs (7.382~8.473) | 3.549 μs (3.180~4.316) | 0.46 (0.39~0.55) | — |
| controller 생성 경로 22개 (Rust) | — | — | 181.98 μs (164.79~185.22) | 73.32 μs (66.61~78.49) | 0.43 (0.36~0.47) | — |
| controller 정상 상태 `plan` 22개 (Rust) | — | — | 394.52 μs (364.27~408.86) | 115.02 μs (107.57~126.79) | 0.30 (0.26~0.32) | — |

작업 정의:

- **convert**: 순수 변환. Go `translate.Convert`는 typed `[]HTTPRoute`, Rust는 변경 전 `Vec<Value>`, 변경 후 `Vec<HttpRoute>`를 반환합니다.
- **+ JSON bytes**: 변환 결과 전체를 JSON bytes로 직렬화합니다. Go는 `json.Marshal`(typed client가 요청 본문을 만들 때와 같은 인코딩), Rust는 `serde_json::to_vec`입니다. Go 출력에는 빈 `status`와 null `creationTimestamp`가 더 있어 마지막 입력 기준 906/911 bytes, Rust는 880/885 bytes입니다. Go는 약 3% 더 많은 바이트를 씁니다.
- **controller 생성 경로**(Rust만): `Action::Create`에 들어가는 실제 `Value`를 만들고 bytes로 직렬화합니다. 변경 전은 convert 결과 그대로, 변경 후는 `to_value` 후 직렬화입니다. kube client의 `DynamicObject` 변환·HTTP 전송은 포함하지 않습니다. Go controller의 같은 경로는 측정하지 않았으므로 Go 열은 비웠습니다.
- **정상 상태 plan**(Rust만): 22개 Ingress와 이미 일치하는 HTTPRoute 22개가 있는 snapshot에서 `plan`을 호출합니다. Ingress·Service 역직렬화, 변환, 기존 route 비교를 포함하며 결과 action은 0개입니다. 주기적 resync에서 반복되는 작업입니다.

**절대 변화:** convert22는 145.4μs → 19.1μs(약 126μs 감소), 정상 상태 plan 22개는 394.5μs → 115.0μs(약 280μs 감소)입니다. 둘 다 컨트롤러가 1회 resync에 쓰는 CPU 시간 중 일부이며 사용자 요청 경로에는 없습니다. 원래도 작았던 비용이 더 작아진 것입니다.

**Go와 비교:** 같은 입력에서 Rust 변경 후 convert는 Go GC 켬보다 짧았습니다(convert1 중앙값 0.16배, convert22 0.40배). 다만 Rust의 변경 후 결과는 입력 문자열을 빌리고, Go는 문자열 header를 복사할 뿐 내용은 공유합니다. 따라서 두 구현의 자료구조가 완전히 같지는 않습니다. API 본문까지 만드는 convert+JSON bytes에서도 0.28~0.31배였지만, Rust controller가 실제로 거치는 `Value` 경로(생성 경로)는 Go 측을 측정하지 않았으므로 "controller 전체가 Go보다 빠르다"고 주장하지 않습니다.

**GC:** 아래 수치는 측정 구간 전체 합계이며 요청 하나의 정지 시간이 아닙니다. Go GC 켬 convert1(100,000회)은 GC 98회, STW 합계 중앙값 679μs(560~1,000), 가장 긴 단일 STW 13μs(10~37)였습니다. convert22(5,000회)는 108~109회, 690μs(592~794), 최대 12μs(9~19)였습니다. +JSON bytes는 ser1(50,000회) 72회, 808μs(493~1,364), 최대 49μs(21~84)였고, ser22(2,500회)는 80~81회, 558μs(490~754), 최대 22μs(10~105)였습니다. GC 끔은 모두 0회이지만 힙이 해제되지 않아 최대 RSS가 208~310MiB까지 커졌습니다. GC 끔이 일관되게 빠르지도 않았습니다(convert1 0.997배, convert22 1.03배 중앙값). Rust에는 tracing GC가 없지만 할당·해제 비용은 측정 시간에 포함됩니다.

### 한계와 잡음

- 공유 VM입니다. 측정 중 1분 load average가 3.4~5.7이었고(실행마다 `paired.json`에 기록), CPU 2를 다른 프로세스가 쓰는 것을 막지 않았습니다.
- 변경 후 Rust의 짧은 작업은 프로세스 단위로 크게 흔들렸습니다. 예를 들어 convert1이 약 0.25μs와 0.83μs 두 무리로 나뉘었습니다. 측정 후 확인에서 ASLR 끔(`setarch -R`)과 기본 상태 모두 비슷하게 흔들려 주소 배치보다는 호스트 상태 영향으로 봅니다. 같은 순서로 실행한 변경 전·Go는 상대적으로 덜 흔들렸습니다([noise-checks.txt](benchmarks/results/noise-checks.txt)). 그래서 중앙값과 함께 범위를 표기하고, 좋은 값을 고르기 위한 재실행은 하지 않았습니다.
- class 조회 코드는 바꾸지 않았습니다. class의 전/후 차이(0.73배, 범위 0.61~1.34)는 잡음으로 보며 개선으로 주장하지 않습니다.
- 최대 RSS는 `wait4`의 `ru_maxrss`입니다. Linux에서 exec 전 fork된 Python 부모의 값이 남아 약 13.3MiB 아래는 측정되지 않습니다. 그 이상인 GC 끔만 의미가 있습니다.
- 단계별 진단은 같은 프로세스에서 번갈아 잰 값이라 headline 표와 절대값이 조금 다릅니다. 할당 계수와 프로파일러가 있는 바이너리는 headline에 쓰지 않았습니다.
- kube client의 `DynamicObject` 변환, HTTP 전송, watch/snapshot 조회, 운영 클러스터 동작은 측정하지 않았습니다.

### 재현

[환경](benchmarks/results/environment-paired.json), [전체 원시 결과](benchmarks/results/paired.json), [콘솔 출력](benchmarks/results/paired-console.txt), [진단 결과](benchmarks/results/diag/).

```sh
python3 docs/benchmarks/build.py /path/to/original-go-source 3977e61   # micro-go, micro-rust, micro-rust-before
python3 docs/benchmarks/run_paired.py docs/benchmarks/results/paired.json [기존 공개 harness 바이너리]
python3 docs/benchmarks/summarize_paired.py docs/benchmarks/results/paired.json
python3 docs/benchmarks/run_diag.py   # 할당 계수, pprof, 단계별 시간 → results/diag/
```

`build.py`는 변경 전 revision을 `docs/benchmarks/before/`(git 무시)에 `git archive`로 풀어 빌드합니다. `run_paired.py`는 출력 경로를 덮어쓰므로 보존할 결과는 먼저 복사하세요.

## 최초 측정 (변경 전 구현, 보존)

아래는 `serde_json::Value` 출력 구현(`e387e61`)의 최초 공개 측정이며 원문 그대로 둡니다. 원시 결과는 [results/micro.json](benchmarks/results/micro.json)입니다. 당시 "유력한 후속 실험"으로 적은 typed 구조체 전환과 할당 측정은 위 절에서 수행했습니다.

측정일: 2026-10-31. 공유 Linux x86_64 VM(노출 vCPU 10개), Go 1.25.5 / Rust 1.99.1 release. 전용 벤치마크 머신이 아니며 CPU affinity는 다른 작업을 배제하지 않습니다. Go 기본 GC(`GOGC=100`), 동일 Go 바이너리의 GC 끔(`GOGC=off`, `GOMEMLIMIT=off`), Rust를 순서를 회전하며 비교했습니다. GC off는 한정된 실험이며 운영 권장 설정이 아닙니다.

Rust는 tracing GC를 사용하지 않아 해당 GC cycle/STW가 없습니다. 메모리 해제·할당 비용과 OS 스케줄링 지연까지 없어지는 것은 아닙니다. Go GC는 동시 실행되는 부분과 짧은 STW 구간이 있습니다. GC trace의 전체 mark 시간은 STW가 아니므로 정지 시간에 합산하지 않았습니다. [Go GC 가이드](https://go.dev/doc/gc-guide), [측정 버전 runtime 문서](https://github.com/golang/go/blob/go1.25.5/src/runtime/extern.go).

microbenchmark는 CPU 2 하나에 고정하고 Go는 `GOMAXPROCS=1`로 실행했습니다. 동시 GC worker도 같은 CPU 시간을 나눠 씁니다. 프로세스마다 warmup 후 고정 반복 횟수를 측정하고 7회 중앙값을 냈습니다. 입력 준비·JSON 결과 출력·Go의 사전 `runtime.GC()`는 시간 밖입니다. Rust는 결과의 drop을 루프 안에서 수행하며 Go는 측정 중 GC 비용을 포함합니다. 따라서 GC off는 측정 이후의 메모리 회수 비용을 포함하지 않습니다. 최적화에 의한 제거는 Go 전역 sink와 Rust `black_box`로 막았습니다. 출력 샘플의 의미가 같은지도 실행기가 확인합니다. Go allocation 수는 `runtime.MemStats` 차이이고 Rust allocation 수는 측정하지 않았습니다. 첫 회를 포함한 모든 최종 측정값을 공개하며 오차 범위는 신뢰구간이 아닌 관측 최솟값~최댓값입니다.

### 결과 (ns/op; 중앙값과 관측 범위)

| 작업 | Go GC 켬 | Go GC 끔 | Rust |
|---|---:|---:|---:|
| class | 2.414 (2.290~9.081) | 2.447 (2.196~2.547) | 7.817 (7.342~8.529) |
| convert1 | 2,223.858 (2,170.047~2,407.173) | 2,844.066 (2,751.145~2,988.504) | 6,698.710 (6,337.011~7,597.524) |
| convert22 | 49,977.218 (48,452.241~54,764.021) | 61,952.513 (56,415.223~70,244.345) | 155,922.684 (151,588.776~175,955.700) |

`convert22`의 1 op는 합성 Ingress 22개 전체입니다. 운영의 HTTPRoute 수와 크기를 맞춰 본 합성 입력이며 운영 객체나 서비스 주소는 사용하지 않았습니다. 공개 examples/basic.json에서 이름/UID/host만 bench-*로 바꿉니다. 파일 읽기·입력 디코딩·출력 인코딩·Kubernetes API 요청은 측정하지 않습니다. Go 원본 ClassName/Convert와 Rust crate class_name/convert를 직접 호출합니다. Go의 빈 status/null creationTimestamp만 제거한 샘플 변환 결과가 같은지 매 실행 검증합니다.

class는 20,000,000회, convert1은 50,000회, convert22는 5,000회, 사전 warmup은 각 1,000회입니다. class는 할당 0·Go GC 0회여서 GC 효과가 아닙니다. Go GC 켬 convert1은 48~49회, convert22는 108~109회 GC가 일어났고 각각 trial당 STW 합계 중앙값 약 380μs와 910μs였습니다. GC off는 두 경우 모두 측정 중 0회입니다. 이 합계는 모든 반복에 걸친 값이며 요청 한 번의 정지 시간이 아닙니다.

Go GC 켬에서 convert1의 할당은 약 34회/2,776bytes, convert22는 약 748회/61,082bytes per op입니다. Rust는 같은 결과를 범용 JSON Value 객체와 문자열로 구성하는 반면 Go는 전용 HTTPRoute 구조체를 반환합니다. Rust allocation 수/프로파일은 이번에 측정하지 않았으므로 느린 이유를 단정하지 않습니다. 유력한 후속 실험은 Rust 출력을 typed 구조체로 바꾸고 자료구조와 할당 비용을 별도 측정하는 것입니다. GC off의 메모리 누적·캐시/페이지 영향도 분리해 측정하지 않았습니다.

결론은 현재 구현의 이 입력에서 Rust의 변환 시간이 약 3배였다는 것입니다. Rust는 tracing GC가 없지만, 모든 프로그램이 더 빠른 것은 아닙니다. 22개에 약 106μs 차이는 절대 비용이 작고 컨트롤러는 데이터 경로에 없으므로 사용자 웹 서비스가 3배 느려졌다는 의미도 아닙니다. API 읽기/쓰기·watch·reconcile·유휴 메모리/전체 CPU 또는 운영 사용자 지연은 이번 실험 범위가 아닙니다.

### 재현

[환경과 커밋](benchmarks/results/environment.json), [전체 원시 결과](benchmarks/results/micro.json). CPU 2를 사용할 수 있는 Linux에서 Go 1.25.5와 Rust 1.99.1을 준비합니다. 다른 환경에서는 affinity를 조정합니다. 원본 Go 소스는 pinned commit으로 체크아웃한 별도 경로를 전달합니다.

```sh
python3 docs/benchmarks/build.py /path/to/original-go-source
python3 docs/benchmarks/run_micro.py
python3 docs/benchmarks/summarize.py docs/benchmarks/results/micro.json
```

빌드 스크립트는 원본을 수정하지 않고 로컬 reference symlink로 import합니다. Rust는 프로젝트의 release 라이브러리를 링크합니다. 재현 결과는 기본 results/micro.json을 갱신하므로 보관할 기존 결과는 먼저 다른 경로에 복사합니다. 이 개인 프로젝트 측정은 통계적 유의성이나 일반적인 언어 순위를 주장하지 않습니다.
