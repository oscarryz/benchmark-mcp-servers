#!/usr/bin/env python3
"""
Test script for all 4 MCP servers.
Tests: initialize, tools/list, and tools/call for each tool.
Handles both plain JSON and SSE (text/event-stream) responses.
"""

import json
import requests
from datetime import datetime

# All servers use the same endpoint path
SERVERS = {
    "python":  "http://localhost:8082/mcp",
    "go":      "http://localhost:8081/mcp",
    "nodejs":  "http://localhost:8083/mcp",
    "java":    "http://localhost:8080/mcp",
    "rust":    "http://localhost:8084/mcp",
}


def mcp_request(url, method, params=None, request_id=1, session_id=None):
    """Send a JSON-RPC 2.0 request to an MCP server.
    Handles both plain JSON and SSE responses using streaming."""
    payload = {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
    }
    if params:
        payload["params"] = params

    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }
    if session_id:
        headers["Mcp-Session-Id"] = session_id

    # Use stream=True to handle SSE responses that may keep connection open
    resp = requests.post(url, json=payload, headers=headers, timeout=10, stream=True)
    resp.raise_for_status()

    new_session_id = resp.headers.get("Mcp-Session-Id", session_id)
    content_type = resp.headers.get("Content-Type", "")

    if "text/event-stream" in content_type:
        # SSE: read line by line until we get a data: line with JSON
        for line in resp.iter_lines(decode_unicode=True):
            if line and line.startswith("data:"):
                payload_str = line[5:].strip()
                if payload_str:
                    resp.close()
                    return json.loads(payload_str), new_session_id
        resp.close()
        raise ValueError("No data found in SSE response")
    else:
        # Plain JSON
        result = resp.json()
        resp.close()
        return result, new_session_id


def mcp_notify(url, method, session_id=None):
    """Send a JSON-RPC 2.0 notification (no id, no response expected)."""
    payload = {"jsonrpc": "2.0", "method": method}
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
    }
    if session_id:
        headers["Mcp-Session-Id"] = session_id
    try:
        requests.post(url, json=payload, headers=headers, timeout=5, stream=True).close()
    except Exception:
        pass


def print_section(title):
    print(f"\n{'='*70}")
    print(f"  {title}")
    print("=" * 70)


def test_server(name, url):
    """Run full MCP test suite against a single server."""
    print_section(f"{name.upper()} ({url})")
    session_id = None

    try:
        # 1. Initialize
        print("\n1️⃣  initialize")
        result, session_id = mcp_request(url, "initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0.0"},
        }, request_id=1)
        r = result.get("result", {})
        print(f"   ✅ protocol: {r.get('protocolVersion')}")
        print(f"   ✅ server:   {r.get('serverInfo', {}).get('name')}")

        # Send initialized notification
        mcp_notify(url, "notifications/initialized", session_id)

        # 2. tools/list
        print("\n2️⃣  tools/list")
        result, session_id = mcp_request(url, "tools/list", {}, request_id=2, session_id=session_id)
        tools = result.get("result", {}).get("tools", [])
        print(f"   ✅ {len(tools)} tools found:")
        for t in sorted(tools, key=lambda x: x["name"]):
            print(f"      • {t['name']}")

        # 3. tools/call — each tool
        print("\n3️⃣  tools/call")

        # 3a. calculate_fibonacci
        print("\n   📌 calculate_fibonacci(n=10)")
        result, session_id = mcp_request(url, "tools/call", {
            "name": "calculate_fibonacci",
            "arguments": {"n": 10},
        }, request_id=3, session_id=session_id)
        data = json.loads(result["result"]["content"][0]["text"])
        print(f"      result={data['result']}, server_type={data['server_type']}")
        assert data["result"] == 55, f"Expected 55, got {data['result']}"

        # 3b. fetch_external_data
        print("\n   📌 fetch_external_data(endpoint=http://mock-api:1080/api)")
        result, session_id = mcp_request(url, "tools/call", {
            "name": "fetch_external_data",
            "arguments": {"endpoint": "http://mock-api:1080/api"},
        }, request_id=4, session_id=session_id)
        data = json.loads(result["result"]["content"][0]["text"])
        print(f"      status_code={data['status_code']}, time={data['response_time_ms']}ms, server={data['server_type']}")

        # 3c. process_json_data
        print("\n   📌 process_json_data(data={'name':'hello','count':42})")
        result, session_id = mcp_request(url, "tools/call", {
            "name": "process_json_data",
            "arguments": {"data": {"name": "hello", "count": 42}},
        }, request_id=5, session_id=session_id)
        data = json.loads(result["result"]["content"][0]["text"])
        print(f"      transformed={data['transformed_data']}, server={data['server_type']}")
        assert data["transformed_data"]["name"] == "HELLO", f"Expected HELLO, got {data['transformed_data']['name']}"

        # 3d. simulate_database_query
        print("\n   📌 simulate_database_query(query='SELECT 1', delay_ms=50)")
        result, session_id = mcp_request(url, "tools/call", {
            "name": "simulate_database_query",
            "arguments": {"query": "SELECT 1", "delay_ms": 50},
        }, request_id=6, session_id=session_id)
        data = json.loads(result["result"]["content"][0]["text"])
        print(f"      query={data['query']}, delay={data['delay_ms']}ms, server={data['server_type']}")

        print(f"\n   ✅ ALL PASSED for {name.upper()}")
        return True

    except requests.exceptions.ConnectionError:
        print(f"   ❌ CONNECTION ERROR — is {name} running?")
        return False
    except Exception as e:
        print(f"   ❌ {type(e).__name__}: {e}")
        return False


def main():
    print_section("MCP SERVERS — COMPREHENSIVE TEST")
    print(f"  Timestamp: {datetime.now().isoformat()}")

    results = {}
    for name, url in SERVERS.items():
        results[name] = test_server(name, url)

    # Summary
    print_section("SUMMARY")
    for name, ok in results.items():
        print(f"  {'✅' if ok else '❌'} {name}")

    passed = sum(results.values())
    total = len(results)
    print(f"\n  {passed}/{total} servers passed")

    if passed == total:
        print("\n  🎉 ALL SERVERS PASSED!")
    print("=" * 70)


if __name__ == "__main__":
    main()
