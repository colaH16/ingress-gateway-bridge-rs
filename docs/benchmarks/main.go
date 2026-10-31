package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"github.com/colaH16/ingress-gateway-bridge/internal/config"
	"github.com/colaH16/ingress-gateway-bridge/internal/translate"
	corev1 "k8s.io/api/core/v1"
	networkingv1 "k8s.io/api/networking/v1"
	"k8s.io/apimachinery/pkg/types"
	"os"
	"runtime"
	gatewayv1 "sigs.k8s.io/gateway-api/apis/v1"
	"time"
)

type Input struct {
	Ingress  networkingv1.Ingress `json:"ingress"`
	Binding  config.Binding       `json:"binding"`
	Services []corev1.Service     `json:"services"`
}

var routesSink []gatewayv1.HTTPRoute
var classSink string
var bytesSink []byte

func main() {
	kind := flag.String("case", "convert1", "")
	n := flag.Int("n", 50000, "")
	file := flag.String("input", "input.json", "")
	flag.Parse()
	data, e := os.ReadFile(*file)
	if e != nil {
		panic(e)
	}
	var inputs []Input
	size := 1
	if *kind == "convert22" || *kind == "ser22" {
		size = 22
	}
	// ser* adds the JSON encoding a typed client performs for each API request body.
	ser := *kind == "ser1" || *kind == "ser22"
	for i := 0; i < size; i++ {
		var in Input
		if e = json.Unmarshal(data, &in); e != nil {
			panic(e)
		}
		in.Ingress.Name = fmt.Sprintf("bench-%d", i)
		in.Ingress.UID = types.UID(fmt.Sprintf("uid-%d", i))
		in.Ingress.Spec.Rules[0].Host = fmt.Sprintf("bench-%d.example.test", i)
		inputs = append(inputs, in)
	}
	services := map[types.NamespacedName]*corev1.Service{}
	op := func() {
		if *kind == "class" {
			classSink, e = translate.ClassName(&inputs[0].Ingress)
			if e != nil {
				panic(e)
			}
		} else {
			for i := range inputs {
				in := &inputs[i]
				routesSink, e = translate.Convert(&in.Ingress, in.Binding, services, config.Config{ControllerName: config.ControllerName})
				if e != nil {
					panic(e)
				}
				if ser {
					if bytesSink, e = json.Marshal(routesSink); e != nil {
						panic(e)
					}
				}
			}
		}
	}
	for i := 0; i < 1000; i++ {
		op()
	}
	runtime.GC()
	var a, b runtime.MemStats
	runtime.ReadMemStats(&a)
	start := time.Now()
	if *kind == "class" {
		for i := 0; i < *n; i++ {
			classSink, e = translate.ClassName(&inputs[0].Ingress)
			if e != nil {
				panic(e)
			}
		}
	} else if ser {
		for i := 0; i < *n; i++ {
			for j := range inputs {
				in := &inputs[j]
				routesSink, e = translate.Convert(&in.Ingress, in.Binding, services, config.Config{ControllerName: config.ControllerName})
				if e != nil {
					panic(e)
				}
				if bytesSink, e = json.Marshal(routesSink); e != nil {
					panic(e)
				}
			}
		}
	} else {
		for i := 0; i < *n; i++ {
			for j := range inputs {
				in := &inputs[j]
				routesSink, e = translate.Convert(&in.Ingress, in.Binding, services, config.Config{ControllerName: config.ControllerName})
				if e != nil {
					panic(e)
				}
			}
		}
	}
	elapsed := time.Since(start)
	runtime.ReadMemStats(&b)
	// Longest stop-the-world pause among cycles in the window (ring buffer keeps 256).
	maxPause := uint64(0)
	for c := b.NumGC; c > a.NumGC && b.NumGC-c < 256; c-- {
		maxPause = max(maxPause, b.PauseNs[(c+255)%256])
	}
	var sample any = map[string]any{"routes": routesSink}
	if ser {
		var decoded []any
		if e = json.Unmarshal(bytesSink, &decoded); e != nil {
			panic(e)
		}
		sample = map[string]any{"routes": decoded, "bytes": len(bytesSink)}
	}
	json.NewEncoder(os.Stdout).Encode(map[string]any{"case": *kind, "iterations": *n, "ns_per_op": float64(elapsed.Nanoseconds()) / float64(*n), "allocs_per_op": float64(b.Mallocs-a.Mallocs) / float64(*n), "bytes_per_op": float64(b.TotalAlloc-a.TotalAlloc) / float64(*n), "gc_cycles": b.NumGC - a.NumGC, "gc_pause_total_us": float64(b.PauseTotalNs-a.PauseTotalNs) / 1000, "gc_pause_max_us": float64(maxPause) / 1000, "gc_cpu_fraction_process": b.GCCPUFraction, "sample": sample, "class": classSink})
}
