"""Exercise real macOS signing + deferred installation in disposable bundles.
The signed 'v004' payload is a deliberate test fixture containing the current
v003 executable. Nothing is uploaded or installed into a real application.
"""
import fcntl
import hashlib
import json
import plistlib
import shutil
import subprocess
import tempfile
import time
from pathlib import Path
from cryptography.hazmat.primitives.serialization import load_pem_private_key

repo=Path(__file__).resolve().parents[1]
binary=repo/"target/release/dramafactory"
key=load_pem_private_key((Path.home()/"Library/Application Support/studio.DramaFactory.DramaFactory/signing/update-private.pem").read_bytes(),password=None)

def sign(path):
    subprocess.run(["/usr/bin/codesign","--force","--sign","-",str(path)],check=True,capture_output=True)

def verify(path):
    subprocess.run(["/usr/bin/codesign","--verify","--deep","--strict",str(path)],check=True,capture_output=True)

def fixture(root):
    target=root/"Fixture.app/Contents/MacOS/DramaFactory"
    target.parent.mkdir(parents=True)
    shutil.copy2(binary,target)
    info=target.parent.parent/"Info.plist"
    info.write_bytes(plistlib.dumps(dict(CFBundleExecutable="DramaFactory",CFBundleIdentifier="studio.dramafactory.updater-fixture",CFBundlePackageType="APPL",CFBundleVersion="3",CFBundleShortVersionString="3")))
    resources=info.parent/"Resources";resources.mkdir();(resources/"fixture.txt").write_text("Disposable sealed resource")
    sign(info.parent.parent);verify(info.parent.parent)
    staged=root/"DramaFactory-v004-aarch64-apple-darwin";shutil.copy2(binary,staged)
    content=staged.read_bytes()
    manifest=dict(version="v004",target="aarch64-apple-darwin",binary=staged.name,sha256=hashlib.sha256(content).hexdigest(),size=len(content))
    message="".join(str(manifest[k])+"\n" for k in ["version","target","binary","sha256","size"]).encode()
    manifest["signature"]=key.sign(message).hex()
    path=root/"manifest.json";path.write_text(json.dumps(manifest))
    return target,info,path,staged,manifest

with tempfile.TemporaryDirectory(prefix="df-update-test-") as temporary:
    root=Path(temporary)
    target,info,path,staged,manifest=fixture(root)
    original=target.read_bytes();content=staged.read_bytes()
    staged.write_bytes(content+b"tampered")
    result=subprocess.run([str(target),"--install-staged-update",str(path),str(root/"studio.sqlite")],capture_output=True)
    assert result.returncode!=0
    assert target.read_bytes()==original
    staged.write_bytes(content)
    lock=open(root/"worker.lock","w+");fcntl.flock(lock,fcntl.LOCK_EX)
    subprocess.run([str(target),"--install-staged-update",str(path),str(root/"studio.sqlite")],check=True)
    time.sleep(.3)
    assert target.read_bytes()==original,"Installer ignored the active worker lock"
    helper=next(root.glob("installer-*"));verify(helper)
    assert subprocess.check_output([str(helper),"--version"],text=True).strip()=="v003","Copied helper cannot launch outside the sealed bundle"
    fcntl.flock(lock,fcntl.LOCK_UN);lock.close()
    deadline=time.monotonic()+15
    while not (root/"installed.json").exists() and time.monotonic()<deadline:time.sleep(.1)
    assert (root/"installed.json").exists(),(root/"install.log").read_text()
    assert (root/"previous-v003").read_bytes()==original
    assert plistlib.loads(info.read_bytes())["CFBundleVersion"]=="4"
    verify(info.parent.parent)
    receipt=json.loads((root/"installed.json").read_text())
    assert receipt["release"]["sha256"]==manifest["sha256"]
    assert receipt["installed_executable_sha256"]==hashlib.sha256(target.read_bytes()).hexdigest()
    assert subprocess.check_output([str(target),"--version"],text=True).strip()=="v003"
    # Independently verify the actual helper waits for its parent to exit.
    second=root/"parent-test";second.mkdir()
    t2,i2,m2,s2,_=fixture(second);before=t2.read_bytes()
    parent=subprocess.Popen(["/bin/sleep","1"])
    installer=subprocess.Popen([str(helper),"--apply-update",str(m2),str(t2),str(second/"studio.sqlite"),str(parent.pid)])
    time.sleep(.3);assert t2.read_bytes()==before,"Installer interrupted its parent"
    parent.wait();assert installer.wait(timeout=10)==0
    verify(i2.parent.parent)
    print("Updater integration passed: tamper rejection, actual copied helper launch, parent-exit deferral, render-lock deferral, sealed-bundle re-signing, preserved external backups, version update and successful executable launch. No real app was changed.")
