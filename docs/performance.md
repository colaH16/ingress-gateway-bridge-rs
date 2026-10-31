# ingress-gateway-bridge-rs 성능 실험

측정일: 2026-10-31. 공유 Linux x86_64 VM(노출 vCPU 10개), Go 1.25.5 / Rust 1.99.1 release. 전용 벤치마크 머신이 아니며 CPU affinity는 다른 작업을 배제하지 않습니다. Go 기본 GC(`GOGC=100`), 동일 Go 바이너리의 GC 끔(`GOGC=off`, `GOMEMLIMIT=off`), Rust를 순서를 회전하며 비교했습니다. GC off는 한정된 실험이며 운영 권장 설정이 아닙니다.

Rust는 tracing GC를 사용하지 않아 해당 GC cycle/STW가 없습니다. 메모리 해제·할당 비용과 OS 스케줄링 지연까지 없어지는 것은 아닙니다. Go GC는 동시 실행되는 부분과 짧은 STW 구간이 있습니다. GC trace의 전체 mark 시간은 STW가 아니므로 정지 시간에 합산하지 않았습니다. [Go GC 가이드](https://go.dev/doc/gc-guide), [측정 버전 runtime 문서](https://github.com/golang/go/blob/go1.25.5/src/runtime/extern.go).

microbenchmark는 CPU 2 하나에 고정하고 Go는 `GOMAXPROCS=1`로 실행했습니다. 동시 GC worker도 같은 CPU 시간을 나눠 씁니다. 프로세스마다 warmup 후 고정 반복 횟수를 측정하고 7회 중앙값을 냈습니다. 입력 준비·JSON 결과 출력·Go의 사전 `runtime.GC()`는 시간 밖입니다. Rust는 결과의 drop을 루프 안에서 수행하며 Go는 측정 중 GC 비용을 포함합니다. 따라서 GC off는 측정 이후의 메모리 회수 비용을 포함하지 않습니다. 최적화에 의한 제거는 Go 전역 sink와 Rust `black_box`로 막았습니다. 출력 샘플의 의미가 같은지도 실행기가 확인합니다. Go allocation 수는 `runtime.MemStats` 차이이고 Rust allocation 수는 측정하지 않았습니다. 첫 회를 포함한 모든 최종 측정값을 공개하며 오차 범위는 신뢰구간이 아닌 관측 최솟값~최댓값입니다.

## 결과 (ns/op; 중앙값과 관측 범위)

| 작업 | Go GC 켬 | Go GC 끔 | Rust |
|---|---:|---:|---:|
| class | 2.414 (2.290~9.081) | 2.447 (2.196~2.547) | 7.817 (7.342~8.529) |
| convert1 | 2,223.858 (2,170.047~2,407.173) | 2,844.066 (2,751.145~2,988.504) | 6,698.710 (6,337.011~7,597.524) |
| convert22 | 49,977.218 (48,452.241~54,764.021) | 61,952.513 (56,415.223~70,244.345) | 155,922.684 (151,588.776~175,955.700) |

`convert22`의 1 op는 합성 Ingress 22개 전체입니다. 운영의 HTTPRoute 수와 크기를 맞춰 본 합성 입력이며 운영 객체나 서비스 주소는 사용하지 않았습니다. 공개 examples/basic.json에서 이름/UID/host만 bench-*로 바꿉니다. 파일 읽기·입력 디코딩·출력 인코딩·Kubernetes API 요청은 측정하지 않습니다. Go 원본 ClassName/Convert와 Rust crate class_name/convert를 직접 호출합니다. Go의 빈 status/null creationTimestamp만 제거한 샘플 변환 결과가 같은지 매 실행 검증합니다.

class는 20,000,000회, convert1은 50,000회, convert22는 5,000회, 사전 warmup은 각 1,000회입니다. class는 할당 0·Go GC 0회여서 GC 효과가 아닙니다. Go GC 켬 convert1은 48~49회, convert22는 108~109회 GC가 일어났고 각각 trial당 STW 합계 중앙값 약 380μs와 910μs였습니다. GC off는 두 경우 모두 측정 중 0회입니다. 이 합계는 모든 반복에 걸친 값이며 요청 한 번의 정지 시간이 아닙니다.

Go GC 켬에서 convert1의 할당은 약 34회/2,776bytes, convert22는 약 748회/61,082bytes per op입니다. Rust는 같은 결과를 범용 JSON Value 객체와 문자열로 구성하는 반면 Go는 전용 HTTPRoute 구조체를 반환합니다. Rust allocation 수/프로파일은 이번에 측정하지 않았으므로 느린 이유를 단정하지 않습니다. 유력한 후속 실험은 Rust 출력을 typed 구조체로 바꾸고 자료구조와 할당 비용을 별도 측정하는 것입니다. GC off의 메모리 누적·캐시/페이지 영향도 분리해 측정하지 않았습니다.

결론은 현재 구현의 이 입력에서 Rust의 변환 시간이 약 3배였다는 것입니다. Rust는 tracing GC가 없지만, 모든 프로그램이 더 빠른 것은 아닙니다. 22개에 약 106μs 차이는 절대 비용이 작고 컨트롤러는 데이터 경로에 없으므로 사용자 웹 서비스가 3배 느려졌다는 의미도 아닙니다. API 읽기/쓰기·watch·reconcile·유휴 메모리/전체 CPU 또는 운영 사용자 지연은 이번 실험 범위가 아닙니다.

## 재현

[환경과 커밋](benchmarks/results/environment.json), [전체 원시 결과](benchmarks/results/micro.json). CPU 2를 사용할 수 있는 Linux에서 Go 1.25.5와 Rust 1.99.1을 준비합니다. 다른 환경에서는 affinity를 조정합니다. 원본 Go 소스는 pinned commit으로 체크아웃한 별도 경로를 전달합니다.

```sh
python3 docs/benchmarks/build.py /path/to/original-go-source
python3 docs/benchmarks/run_micro.py
python3 docs/benchmarks/summarize.py docs/benchmarks/results/micro.json
```

빌드 스크립트는 원본을 수정하지 않고 로컬 reference symlink로 import합니다. Rust는 프로젝트의 release 라이브러리를 링크합니다. 재현 결과는 기본 results/micro.json을 갱신하므로 보관할 기존 결과는 먼저 다른 경로에 복사합니다. 이 개인 프로젝트 측정은 통계적 유의성이나 일반적인 언어 순위를 주장하지 않습니다.
