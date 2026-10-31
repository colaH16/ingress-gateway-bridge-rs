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
	if *kind == "convert22" {
		size = 22
	}
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
	json.NewEncoder(os.Stdout).Encode(map[string]any{"case": *kind, "iterations": *n, "ns_per_op": float64(elapsed.Nanoseconds()) / float64(*n), "allocs_per_op": float64(b.Mallocs-a.Mallocs) / float64(*n), "bytes_per_op": float64(b.TotalAlloc-a.TotalAlloc) / float64(*n), "gc_cycles": b.NumGC - a.NumGC, "gc_pause_total_us": float64(b.PauseTotalNs-a.PauseTotalNs) / 1000, "sample": map[string]any{"routes": routesSink}, "class": classSink})
}
