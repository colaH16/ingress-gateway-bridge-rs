"""Usage: summarize_paired.py results/paired.json

Per case and variant: median/min/max ns/op over all rounds, plus per-round ratios
(each ratio pairs two variants from the same round) reported as median [min, max].
GC counts and stop-the-world pauses are whole-trial totals per process, not per op.
"""
import json,statistics,pathlib,sys
rows=json.loads(pathlib.Path(sys.argv[1]).read_text())
def stat(xs):return {'median':statistics.median(xs),'min':min(xs),'max':max(xs),'runs':len(xs)}
summary={}
for case in dict.fromkeys(r['case'] for r in rows):
 by={}
 for r in rows:
  if r['case']==case:by.setdefault(r['variant'],{})[r['round']]=r
 s={'variants':{},'ratios':{}}
 for variant,rs in by.items():
  v={'ns_per_op':stat([r['ns_per_op'] for r in rs.values()]),'peak_rss_mib':stat([r['peak_rss_mib'] for r in rs.values()])}
  for key in ['gc_cycles','gc_pause_total_us','gc_pause_max_us','allocs_per_op','bytes_per_op','serialized_bytes']:
   if key in next(iter(rs.values())):v[key]=stat([r[key] for r in rs.values()])
  s['variants'][variant]=v
 for a,b in [('rust_after','rust_before'),('rust_after','go_gc'),('rust_before','go_gc'),('go_off','go_gc'),('rust_published','rust_before')]:
  if a in by and b in by:
   s['ratios'][f'{a}/{b}']=stat([by[a][k]['ns_per_op']/by[b][k]['ns_per_op'] for k in by[a] if k in by[b]])
 summary[case]=s
print(json.dumps(summary,indent=1))
def fmt(ns):return f'{ns/1000:.3f} us' if ns>=1000 else f'{ns:.2f} ns'
for case,s in summary.items():
 print(f'\n{case}',file=sys.stderr)
 for variant,v in s['variants'].items():
  t=v['ns_per_op'];extra=''
  if 'gc_cycles' in v:extra=f" gc={v['gc_cycles']['median']:.0f} stw_total={v['gc_pause_total_us']['median']:.0f}us max={v['gc_pause_max_us']['median']:.0f}us"
  print(f"  {variant:15} {fmt(t['median'])} [{fmt(t['min'])}, {fmt(t['max'])}] rss={v['peak_rss_mib']['median']:.1f}MiB{extra}",file=sys.stderr)
 for k,r in s['ratios'].items():
  print(f"  {k:26} {r['median']:.3f} [{r['min']:.3f}, {r['max']:.3f}]",file=sys.stderr)
