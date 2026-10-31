"""Usage: build.py GO_REFERENCE_CHECKOUT [BEFORE_REV]

Builds micro-go and micro-rust (working tree). With BEFORE_REV, also extracts that
revision with `git archive` into before/ and builds micro-rust-before from the same
main.rs with `--cfg bridge_before`, for paired before/after runs.
"""
import os,pathlib,subprocess,sys
root=pathlib.Path(__file__).resolve().parent
repo=root.parent.parent
source=pathlib.Path(sys.argv[1]).resolve()
def run(cmd,cwd=root):subprocess.run(cmd,cwd=cwd,check=True)
# The reference must be the documented pinned commit. Never modify its source.
expected='41b4b395f37abd0b03ba858f1da6c32c02834813'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==expected
ref=root/'reference'
if ref.exists() or ref.is_symlink():
 assert ref.resolve()==source, 'Existing reference points elsewhere'
else:ref.symlink_to(source,target_is_directory=True)
os.environ['GOTOOLCHAIN']='go1.25.5'
run(['go','build','-o','micro-go','.'])
def harness(crate,target,out,extra=()):
 run(['cargo','build','--locked','--release','--lib','--target-dir',str(target)],crate)
 deps=target/'release/deps'
 def lib(pattern):return str(max(deps.glob(pattern),key=lambda p:p.stat().st_mtime))
 run(['rustc','--edition=2024','-O','main.rs','-L','dependency='+str(deps),'--extern','ingress_gateway_bridge_rs='+lib('libingress_gateway_bridge_rs-*.rlib'),'--extern','serde_json='+lib('libserde_json-*.rlib'),'--check-cfg','cfg(bridge_before)',*extra,'-o',out])
harness(repo,repo/'target','micro-rust')
if len(sys.argv)>2:
 before=root/'before'
 rev=subprocess.check_output(['git','rev-parse',sys.argv[2]],cwd=repo,text=True).strip()
 stamp=before/'REVISION'
 if not stamp.exists() or stamp.read_text().strip()!=rev:
  subprocess.run(['rm','-rf',str(before/'src')],check=True);(before/'src').mkdir(parents=True)
  archive=subprocess.run(['git','archive',rev],cwd=repo,check=True,capture_output=True).stdout
  subprocess.run(['tar','-x','-C',str(before/'src')],input=archive,check=True)
  stamp.write_text(rev+'\n')
 harness(before/'src',before/'target','micro-rust-before',['--cfg','bridge_before'])
