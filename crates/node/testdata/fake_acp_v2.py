"""A deterministic draft ACP v2 peer for session lifecycle tests."""

import json
import sys

active = False
session = "session-test"
sequence = 0
log_path = sys.argv[1]

options = [
    {"configId": "permission-mode", "name": "Mode", "category": "mode", "type": "select",
     "currentValue": "agent", "options": [
         {"value": "agent", "name": "Agent"}, {"value": "plan", "name": "Plan"},
         {"value": "bypassPermissions", "name": "Full access"}]},
    {"configId": "model-picker", "name": "Model", "category": "model", "type": "select",
     "currentValue": "test", "options": [{"value": "test", "name": "Test"}, {"value": "other", "name": "Other"}]},
]


def send(message):
    print(json.dumps({"jsonrpc": "2.0", **message}), flush=True)


def update(body):
    send({"method": "session/update", "params": {"sessionId": session, "update": body}})


def state(value, **fields):
    update({"sessionUpdate": "state_update", "state": value, **fields})


def reply(request, result):
    send({"id": request["id"], "result": result})


for line in sys.stdin:
    message = json.loads(line)
    with open(log_path, "a") as log:
        log.write(line)
    method = message.get("method")
    params = message.get("params", {})
    if method == "initialize":
        assert params["protocolVersion"] == 2
        # Legacy capability fields are ignored when v2 is selected.
        assert params["clientCapabilities"]["session"]["compaction"] == {}
        reply(message, {"protocolVersion": 2, "info": {"name": "test-agent", "version": "1"},
                        "capabilities": {"session": {}}})
    elif method in ("session/new", "session/resume"):
        if method == "session/resume":
            assert params["replayFrom"] == {"type": "start"}
            session = params["sessionId"]
            update({"sessionUpdate": "user_message", "messageId": "history-user",
                    "content": [{"type": "text", "text": "earlier prompt"}]})
            if session == "active-session":
                active = True
                state("running")
            else:
                state("idle")
        reply(message, {"sessionId": session, "configOptions": options})
    elif method == "session/set_config_option":
        assert params["type"] == "id"
        for option in options:
            if option["configId"] == params["configId"]:
                option["currentValue"] = params["value"]
                break
        else:
            raise AssertionError("unknown option")
        reply(message, {"configOptions": options})
    elif method == "session/prompt":
        assert not active, "a second prompt arrived before idle"
        sequence += 1
        text = " ".join(block.get("text", "") for block in params["prompt"])
        user = "user-" + str(sequence)
        state("running")
        if text == "reject":
            send({"id": message["id"], "error": {"code": -32602, "message": "rejected input"}})
            state("idle", stopReason="error", error={"code": -32602, "message": "rejected input"})
            continue
        # Echo before acceptance to exercise reconciliation without matching text.
        update({"sessionUpdate": "user_message", "messageId": user,
                "content": [{"type": "text", "text": text}]})
        if text != "late":
            reply(message, {"messageId": user})
        if text == "wait":
            active = True
            state("requires_action")
            send({"id": "permission-1", "method": "session/request_permission", "params": {
                "sessionId": session, "title": "Choose a file", "description": "Pick one to continue",
                "options": [{"optionId": "one", "kind": "allow_once", "name": "One"},
                            {"optionId": "two", "kind": "allow_once", "name": "Two"}]}})
        else:
            update({"sessionUpdate": "agent_message_chunk", "messageId": "answer-" + str(sequence),
                    "content": {"type": "text", "text": "initial"}})
            update({"sessionUpdate": "agent_message", "messageId": "answer-" + str(sequence),
                    "content": [{"type": "text", "text": "Done"}]})
            if text == "error":
                state("idle", stopReason="error", error={"code": -32603, "message": "work failed"})
            elif text == "unknown":
                state("idle", stopReason="_future_reason")
            elif text == "missing":
                state("idle")
            else:
                state("idle", stopReason="end_turn", usage={"totalTokens": 7, "inputTokens": 4, "outputTokens": 3})
                state("idle", stopReason="end_turn", usage={"totalTokens": 7, "inputTokens": 4, "outputTokens": 3})
            if text == "late":
                reply(message, {"messageId": user})
    elif method == "session/cancel":
        if active:
            active = False
            state("idle", stopReason="cancelled")
    elif message.get("id") == "permission-1":
        if active and message.get("result", {}).get("outcome", {}).get("outcome") != "cancelled":
            active = False
            state("running")
            state("idle", stopReason="end_turn")
    elif method is not None and "id" in message:
        send({"id": message["id"], "error": {"code": -32601, "message": "unknown method"}})
