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
run(['cargo','build','--locked','--release','--lib'],repo)
deps=repo/'target/release/deps'
def lib(pattern):return str(max(deps.glob(pattern),key=lambda p:p.stat().st_mtime))
run(['rustc','--edition=2024','-O','main.rs','-L','dependency='+str(deps),'--extern','ingress_gateway_bridge_rs='+lib('libingress_gateway_bridge_rs-*.rlib'),'--extern','serde_json='+lib('libserde_json-*.rlib'),'-o','micro-rust'])
