"""Usage: run_diag.py [BEFORE_REV]  (run build.py with the same BEFORE_REV first)

Allocation counts, a pprof CPU sample and in-process stage timings for the working tree
("after") and, if before/src exists, the extracted BEFORE_REV ("before"). Writes
results/diag/{after,before}/. Separate from headline timing: the counting allocator
adds atomics to every allocation and the profiler interrupts the process.
"""
import pathlib,shutil,subprocess,sys
root=pathlib.Path(__file__).resolve().parent
inp=str(root/'input.json')
sides={'after':root/'diag'}
if (root/'before/src').exists():
 crate=root/'before/diag'
 shutil.rmtree(crate,ignore_errors=True);shutil.copytree(root/'diag',crate,ignore=shutil.ignore_patterns('target'))
 manifest=crate/'Cargo.toml'
 manifest.write_text(manifest.read_text().replace('path = "../../.."','path = "../src"'))
 sides['before']=crate
def build(crate,features):
 target=crate/'target'
 subprocess.run(['cargo','build','--release','--locked','--target-dir',str(target),*features],cwd=crate,check=True)
 return target/'release'
for side,crate in sides.items():
 out=root/'results/diag'/side;out.mkdir(parents=True,exist_ok=True)
 release=build(crate,[])
 counts=subprocess.check_output(['taskset','-c','2',str(release/'bridge-diag'),inp],text=True)
 stages=subprocess.check_output(['taskset','-c','2',str(release/'bridge-stages'),inp],text=True)
 release=build(crate,['--features','profile'])
 profile=subprocess.check_output(['taskset','-c','2',str(release/'bridge-diag'),inp,'4000000' if side=='after' else '400000',str(out/'flame.svg')],text=True)
 (out/'alloc.jsonl').write_text(counts);(out/'stages.jsonl').write_text(stages)
 # Keep the tallies readable: the 60 hottest inclusive and self symbols.
 lines=profile.splitlines();cut=lines.index(next(l for l in lines if l.startswith('== self')))
 (out/'profile.txt').write_text('\n'.join(lines[:61]+lines[cut:cut+61])+'\n')
 print(side,'\n'+counts+stages,flush=True)
