#!/usr/bin/env python3
"""Local fake Kubernetes API: no cluster access, no writes; requires release controller."""
import http.server,json,os,pathlib,queue,signal,subprocess,tempfile,threading,time,urllib.parse
ROOT=pathlib.Path(__file__).resolve().parent.parent
BINARY=os.environ.get('BRIDGE_BINARY',str(ROOT/'target/release/bridge-controller'))
class State:
 def __init__(self):self.block=False;self.blocked=threading.Event();self.stop=threading.Event();self.watchers=[];self.snapshots=0
state=State()
class Handler(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_GET(self):
  parsed=urllib.parse.urlsplit(self.path);q=urllib.parse.parse_qs(parsed.query)
  resource=parsed.path.rsplit('/',1)[-1]
  kinds={'ingresses':'Ingress','ingressclasses':'IngressClass','services':'Service','httproutes':'HTTPRoute','gateways':'Gateway','namespaces':'Namespace'}
  if q.get('watch')==['true']:
   self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();channel=queue.Queue();state.watchers.append(channel)
   # Streaming list clients require initial bookmark before processing changes.
   if q.get('sendInitialEvents')==['true']:
    self.wfile.write((json.dumps({'type':'BOOKMARK','object':{'apiVersion':'v1','kind':kinds.get(resource,'Namespace'),'metadata':{'resourceVersion':'1','annotations':{'k8s.io/initial-events-end':'true'}}}})+'\n').encode());self.wfile.flush()
   while not state.stop.is_set():
    try:channel.get(timeout=.1)
    except queue.Empty:continue
    try:self.wfile.write((json.dumps({'type':'ADDED','object':{'apiVersion':'v1','kind':kinds.get(resource,'Namespace'),'metadata':{'name':'wake','resourceVersion':str(time.time_ns())}}})+'\n').encode());self.wfile.flush()
    except (BrokenPipeError,ConnectionResetError):break
   return
  if state.block:
   state.blocked.set();state.stop.wait(10);return
  items=[]
  if resource=='ingresses':
   state.snapshots+=1
   items=[{'apiVersion':'networking.k8s.io/v1','kind':'Ingress','metadata':{'name':'missing-parent','namespace':'test','uid':'source','resourceVersion':'1'},'spec':{'ingressClassName':'test','rules':[{'host':'example.test','http':{'paths':[{'path':'/','pathType':'Prefix','backend':{'service':{'name':'app','port':{'number':80}}}}]}}]}}]
  if resource=='ingressclasses':items=[{'apiVersion':'networking.k8s.io/v1','kind':'IngressClass','metadata':{'name':'test'},'spec':{'controller':'cola16.app/ingress-gateway-bridge'}}]
  version='networking.k8s.io/v1' if resource in ('ingresses','ingressclasses') else 'gateway.networking.k8s.io/v1' if resource in ('httproutes','gateways') else 'v1'
  data=json.dumps({'apiVersion':version,'kind':kinds.get(resource,'Namespace')+'List','metadata':{'resourceVersion':'1'},'items':items}).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
def wait(predicate,seconds=5):
 deadline=time.monotonic()+seconds
 while time.monotonic()<deadline:
  if predicate():return
  time.sleep(.03)
 raise AssertionError('runtime condition timeout')
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);server.daemon_threads=True;threading.Thread(target=server.serve_forever,daemon=True).start()
with tempfile.TemporaryDirectory(prefix='bridge-runtime-') as tmp:
 p=pathlib.Path(tmp);(p/'kubeconfig').write_text(json.dumps({'apiVersion':'v1','kind':'Config','clusters':[{'name':'fake','cluster':{'server':f'http://127.0.0.1:{server.server_port}'}}],'contexts':[{'name':'fake','context':{'cluster':'fake','user':'fake'}}],'users':[{'name':'fake','user':{}}],'current-context':'fake'}));(p/'bridge.json').write_text(json.dumps({'controllerName':'cola16.app/ingress-gateway-bridge','classes':{'test':{'parentRefs':[{'name':'absent','namespace':'test'}]}}}))
 with (p/'log').open('w') as log:
  proc=subprocess.Popen([BINARY],env=dict(os.environ,KUBECONFIG=str(p/'kubeconfig'),BRIDGE_CONFIG=str(p/'bridge.json'),BRIDGE_DRY_RUN='true',BRIDGE_HEALTH_PROBE_BIND_ADDRESS='127.0.0.1:0'),stdout=log,stderr=log)
  try:
   wait(lambda:state.snapshots>=1)
   for _ in range(6):
    prior=state.snapshots
    for channel in list(state.watchers):channel.put(True)
    wait(lambda:state.snapshots>prior,3.5)
   # A broken ingress must not build a global 30-second event delay.
   assert 'reconciliation deferred' in (p/'log').read_text(), 'fixture must actually fail planning'
   state.block=True
   for channel in list(state.watchers):channel.put(True)
   assert state.blocked.wait(4),'expected snapshot in flight'
   started=time.monotonic();proc.send_signal(signal.SIGTERM);proc.wait(timeout=2)
   assert proc.returncode==0, (proc.returncode,(p/'log').read_text())
   print('PASS: persistent per-Ingress failure does not delay other watch events; SIGTERM cancels blocked snapshot in',round(time.monotonic()-started,3),'s')
  finally:
   state.stop.set()
   if proc.poll() is None:proc.kill();proc.wait()
server.shutdown();server.server_close()
