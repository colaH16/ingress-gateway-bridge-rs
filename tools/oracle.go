// SPDX-License-Identifier: Apache-2.0
package main
import (
 "encoding/json"
 "errors"
 "os"
 "github.com/colaH16/ingress-gateway-bridge/internal/config"
 "github.com/colaH16/ingress-gateway-bridge/internal/translate"
 corev1 "k8s.io/api/core/v1"
 networkingv1 "k8s.io/api/networking/v1"
 "k8s.io/apimachinery/pkg/types"
)
func main() {
 var input struct {
  Ingress networkingv1.Ingress `json:"ingress"`
  Binding config.Binding `json:"binding"`
  Services []corev1.Service `json:"services"`
  ControllerName string `json:"controllerName"`
 }
 if err := json.NewDecoder(os.Stdin).Decode(&input); err != nil { panic(err) }
 if input.ControllerName == "" { input.ControllerName = config.ControllerName }
 services := map[types.NamespacedName]*corev1.Service{}
 for i := range input.Services { s:= &input.Services[i]; services[types.NamespacedName{Namespace:s.Namespace,Name:s.Name}] = s }
 _, err := translate.ClassName(&input.Ingress)
 var output any
 if err == nil {
  routes, e := translate.Convert(&input.Ingress,input.Binding,services,config.Config{ControllerName:input.ControllerName})
  err=e
  output=map[string]any{"routes":routes}
 }
 if err != nil {
  kind:="dependency"
  var rejected *translate.Rejected
  if errors.As(err,&rejected) { kind="rejected" }
  output=map[string]any{"error":map[string]string{"kind":kind,"message":err.Error()}}
 }
 if err := json.NewEncoder(os.Stdout).Encode(output); err != nil { panic(err) }
}
