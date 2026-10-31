import json,statistics,pathlib,sys
p=pathlib.Path(sys.argv[1]);rows=json.loads(p.read_text())
for case in dict.fromkeys(r['case'] for r in rows):
 print('\n'+case)
 for variant in ['go_gc','go_off','rust']:
  rs=[r for r in rows if r['case']==case and r['variant']==variant]
  if not rs:continue
  keys=['ns_per_op','p50_us','p99_us','rps','server_cpu_us_per_request','peak_rss_mib','gc_cycles','gc_stw_total_us','gc_stw_max_us','gc_pause_total_us','allocs_per_op','bytes_per_op']
  print(variant, 'runs',len(rs),{k:{'median':round(statistics.median(r[k] for r in rs),3),'min':round(min(r[k] for r in rs),3),'max':round(max(r[k] for r in rs),3)} for k in keys if k in rs[0]})
