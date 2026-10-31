import os,json,subprocess,pathlib,sys
root=pathlib.Path(__file__).resolve().parent
project='bridge'
if project=='rustalk':
 cases=[('json',500000),('form',500000)];go=root/'micro-go';rust=root/'micro/target/release/rustalk-microbench';out=root/'results/micro.json'
else:
 cases=[('class',20000000),('convert1',50000),('convert22',5000)];go=root/'micro-go';rust=root/'micro-rust';out=root/'results/micro.json'
variants=['go_gc','go_off','rust'];results=[];samples={}
for round_idx in range(7):
 for case,n in cases:
  for variant in variants[round_idx%3:]+variants[:round_idx%3]:
   env=dict(os.environ,GOMAXPROCS='1',GOMEMLIMIT='off',GOGC='off' if variant=='go_off' else '100',GODEBUG='')
   cmd=[str(rust),case,str(n)] if variant=='rust' else [str(go),'-case',case,'-n',str(n)]
   if project=='bridge':cmd+=([str(root/'input.json')] if variant=='rust' else ['-input',str(root/'input.json')])
   d=json.loads(subprocess.check_output(['taskset','-c','2',*cmd],env=env))
   if project=='bridge' and case!='class':
    v=d['sample']
    for route in v['routes']:
     route.pop('status',None);route['metadata'].pop('creationTimestamp',None)
    if case in samples:assert samples[case]==v
    samples[case]=v
   if project=='rustalk':
    if case=='json': v=json.loads(d['sample_json'])
    else:v={k:(x[0] if isinstance(x,list) else x) for k,x in d['sample_fields'].items()}
    if case in samples:assert samples[case]==v
    samples[case]=v
   for key in ['sample','sample_json','sample_fields','class']:d.pop(key,None)
   if variant=='go_off':assert d['gc_cycles']==0
   d.update(variant=variant,round=round_idx+1);results.append(d);out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps(results,indent=2)+'\n')
   print(project,case,variant,round_idx+1,round(d['ns_per_op'],2),'ns/op',flush=True)
