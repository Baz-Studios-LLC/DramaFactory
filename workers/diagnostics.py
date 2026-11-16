"""Import-only environment probe. Never construct models or download files."""
import contextlib,importlib,importlib.metadata,json,os,platform,sys
specs=json.loads(sys.argv[1])
checks=[]
# Imported packages occasionally print banners. Only our bounded JSON report
# belongs on stdout; no imported diagnostic may contaminate MCP results.
with open(os.devnull,'w') as sink,contextlib.redirect_stdout(sink),contextlib.redirect_stderr(sink):
    for module,distribution,symbol in specs:
        check=dict(module=module,version=None,error=None)
        try:
            imported=importlib.import_module(module)
            if symbol:getattr(imported,symbol)
            check['version']=importlib.metadata.version(distribution)
        except Exception as error:
            check['error']=f'{type(error).__name__}: {error}'[:2000]
        checks.append(check)
print(json.dumps(dict(python_version=platform.python_version(),architecture=platform.machine(),checks=checks)),flush=True)
