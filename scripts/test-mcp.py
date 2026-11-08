"""Exercise actual stdio MCP against an isolated temporary database."""
import json
import os
import subprocess
import tempfile
from pathlib import Path

with tempfile.TemporaryDirectory(prefix="dramafactory-mcp-") as temporary:
    env = dict(os.environ, DRAMAFACTORY_DATABASE=str(Path(temporary) / "studio.sqlite"))
    binary = Path(__file__).resolve().parents[1] / "target/release/dramafactory"
    process = subprocess.Popen([str(binary), "--mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=env)
    sequence = 0

    def request(method, params):
        global sequence
        sequence += 1
        process.stdin.write(json.dumps(dict(jsonrpc="2.0", id=sequence, method=method, params=params)) + "\n")
        process.stdin.flush()
        response = json.loads(process.stdout.readline())
        assert response["id"] == sequence
        return response["result"]

    def tool(name, **arguments):
        response = request("tools/call", dict(name="dramafactory_" + name, arguments=arguments))
        assert not response["isError"], response
        return json.loads(response["content"][0]["text"])

    assert request("initialize", {})["serverInfo"]["name"] == "DramaFactory"
    assert len(request("tools/list", {})["tools"]) == 8
    series = tool("create_series", name="Integration test", premise="Temporary test data")
    episode = tool("create_episode", series_id=series["id"], title="Pilot")
    tool("save_screenplay", episode_id=episode["id"], text="INT. STUDIO — NIGHT")
    assert tool("list_episodes", series_id=series["id"])[0]["screenplay"] == "INT. STUDIO — NIGHT"
    job = tool("queue_render", prompt="A five-second cinematic shot")
    assert job["status"] == "blocked"
    tool("cancel_job", job_id=job["id"])
    assert tool("list_jobs")[0]["status"] == "canceled"
    process.stdin.close()
    assert process.wait(timeout=10) == 0
    print("MCP integration passed: initialize, discovery, series, episode, screenplay, render request, cancellation.")
