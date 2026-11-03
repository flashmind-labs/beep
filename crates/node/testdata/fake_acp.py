"""A stand-in ACP agent for tests: no network, no model.

A prompt with "edit" asks permission to edit notes.txt and reports the edit
when allowed. A prompt with "pick" asks which file to edit instead, as a
choice with nothing to reject, and edits notes.txt when it is picked. A
prompt with "wait" runs until it is cancelled. Anything else is echoed back.
"""

import json
import os
import uuid
import sys

cwd = None
waiting = {}  # request id we sent -> the prompt request it belongs to
next_id = 100


def send(message):
    message["jsonrpc"] = "2.0"
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def update(session, body):
    send({"method": "session/update", "params": {"sessionId": session, "update": body}})


def say(session, text):
    update(session, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})


def done(prompt_id, reason="end_turn"):
    send({"id": prompt_id, "result": {"stopReason": reason}})


def handle(message):
    global cwd, next_id
    method = message.get("method")
    if method == "initialize":
        send({"id": message["id"], "result": {"protocolVersion": 1, "agentCapabilities": {}}})
    elif method == "session/new":
        cwd = message["params"]["cwd"]
        # Unique like a real agent's, as the node goes by it.
        send({"id": message["id"], "result": {"sessionId": f"fake-{uuid.uuid4()}"}})
    elif method == "session/prompt":
        params = message["params"]
        session = params["sessionId"]
        text = params["prompt"][0]["text"]
        update(session, {"sessionUpdate": "plan", "entries": [
            {"content": "Read the prompt", "priority": "high", "status": "completed"},
            {"content": "Answer it", "priority": "high", "status": "in_progress"},
        ]})
        if "edit" in text or "pick" in text:
            path = os.path.join(cwd, "notes.txt")
            update(session, {"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit notes.txt",
                             "kind": "edit", "status": "pending", "locations": [{"path": path}]})
            next_id += 1
            waiting[next_id] = (message["id"], session)
            if "pick" in text:
                title = "Which file?"
                options = [
                    {"optionId": "allow", "name": "notes.txt", "kind": "allow_once"},
                    {"optionId": "other", "name": "other.txt", "kind": "allow_once"},
                ]
            else:
                title = "Edit notes.txt"
                options = [
                    {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                    {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
                ]
            send({"id": next_id, "method": "session/request_permission", "params": {
                "sessionId": session,
                "toolCall": {"toolCallId": "t1", "title": title, "rawInput": {"file_path": path}},
                "options": options,
            }})
        elif "wait" in text:
            update(session, {"sessionUpdate": "tool_call", "toolCallId": "t2", "title": "Wait",
                             "kind": "other", "status": "in_progress"})
            waiting["cancel"] = message["id"]
        else:
            say(session, "Echo: " + text)
            done(message["id"])
    elif method == "session/cancel":
        prompt_id = waiting.pop("cancel", None)
        if prompt_id is not None:
            done(prompt_id, "cancelled")
    elif "id" in message and message["id"] in waiting:
        prompt_id, session = waiting.pop(message["id"])
        outcome = message.get("result", {}).get("outcome", {})
        if outcome.get("optionId") == "allow":
            update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"})
            say(session, "Edited notes.txt")
        else:
            update(session, {"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "failed"})
            say(session, "Left notes.txt alone")
        done(prompt_id)
    elif "id" in message and method is not None:
        send({"id": message["id"], "error": {"code": -32601, "message": "unknown"}})


print("fake agent starting", file=sys.stderr)
for line in sys.stdin:
    if line.strip():
        handle(json.loads(line))
