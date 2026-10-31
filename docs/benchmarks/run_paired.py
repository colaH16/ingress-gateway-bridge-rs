"""Paired, rotated runs: Go GOGC=100, Go GOGC=off, Rust before (micro-rust-before) and
Rust after (micro-rust). Usage: run_paired.py OUT.json [PUBLISHED_RUST_BINARY]

Every round runs every case once per variant, rotating variant order. All rounds are kept;
nothing is re-run selectively. Samples are compared across variants (Go-only fields
status/creationTimestamp removed) so a faster variant cannot be doing different work.
The optional published binary (the harness of results/micro.json) checks harness changes.
"""
import json,os,pathlib,subprocess,sys
root=pathlib.Path(__file__).resolve().parent
out=pathlib.Path(sys.argv[1]);published=sys.argv[2] if len(sys.argv)>2 else None
inp=str(root/'input.json');rounds=7
# GOGC=off heap grows to iterations*bytes_per_op (bounded: <=~0.5 GiB per process here).
cases=[('class',20000000),('convert1',100000),('convert22',5000),('ser1',50000),('ser22',2500),('create1',100000),('create22',5000),('plan22',2000)]
go_cases={'class','convert1','convert22','ser1','ser22'}
def variants(case):
 v=['go_gc','go_off','rust_before','rust_after'] if case in go_cases else ['rust_before','rust_after']
 if published and case in ('class','convert1','convert22'):v.append('rust_published')
 return v
def command(variant,case,n):
 if variant.startswith('go'):return [str(root/'micro-go'),'-case',case,'-n',str(n),'-input',inp]
 binary={'rust_before':str(root/'micro-rust-before'),'rust_after':str(root/'micro-rust'),'rust_published':published}[variant]
 return [binary,case,str(n),inp]
def normalize(sample):
 sample=json.loads(json.dumps(sample));sample.pop('bytes',None)
 for route in sample.get('routes') or []:
  route.pop('status',None);route['metadata'].pop('creationTimestamp',None)
 return sample
results=[];samples={}
for round_idx in range(rounds):
 for case,n in cases:
  vs=variants(case);k=round_idx%len(vs)
  for variant in vs[k:]+vs[:k]:
   env=dict(os.environ,GOMAXPROCS='1',GOMEMLIMIT='off',GOGC='off' if variant=='go_off' else '100',GODEBUG='')
   load=os.getloadavg()[0]
   p=subprocess.Popen(['taskset','-c','2',*command(variant,case,n)],env=env,stdout=subprocess.PIPE)
   raw=p.stdout.read();_,status,usage=os.wait4(p.pid,0)
   assert status==0,(variant,case,status)
   d=json.loads(raw)
   if case!='class':
    v=normalize(d['sample'])
    if case in samples:assert samples[case]==v,(case,variant)
    samples[case]=v
   if 'bytes' in d['sample']:d['serialized_bytes']=d['sample']['bytes']
   for key in ['sample','class']:d.pop(key,None)
   if variant=='go_off':assert d['gc_cycles']==0
   d.update(variant=variant,round=round_idx+1,peak_rss_mib=usage.ru_maxrss/1024,process_cpu_s=usage.ru_utime+usage.ru_stime,loadavg1_before=load)
   results.append(d);out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps(results,indent=2)+'\n')
   print(case,variant,round_idx+1,round(d['ns_per_op'],2),'ns/op',flush=True)
