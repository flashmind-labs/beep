"""Prints the arguments of app tools as the agent sees them.

    just tool-schema GMAIL_MOVE_TO_TRASH FASTMAIL_DELETE_EMAIL
    just tool-schema --list outlook

Composio tools come from Composio's v3.1 API, the version flashmind calls.
Fastmail tools come from the schema the server cached in `mcp_servers`, so
the server must have connected Fastmail once. Keys are read from the server
config given with --config.
"""

import argparse
import json
import sys
import tomllib

import httpx

COMPOSIO = "https://backend.composio.dev/api/v3.1"


def composio(key, path):
    response = httpx.get(f"{COMPOSIO}/{path}", headers={"x-api-key": key}, timeout=30)
    if response.is_error:
        return {"error": f"{response.status_code} {response.reason_phrase}"}
    return response.json()


def fastmail_tools(database_url):
    import psycopg

    with psycopg.connect(database_url) as db:
        row = db.execute(
            "select config->'cached_tools' from mcp_servers "
            "where name = 'fastmail' and config->'cached_tools' is not null limit 1"
        ).fetchone()
    return {tool["name"]: tool for tool in (row[0] if row else [])}


def show(name, schema, description=""):
    print(f"## {name}")
    if description:
        print(f"   {description.strip().splitlines()[0][:160]}")
    required = set(schema.get("required") or [])
    for arg, spec in (schema.get("properties") or {}).items():
        mark = "*" if arg in required else " "
        kind = spec.get("type", "")
        about = (spec.get("description") or "").replace("\n", " ")[:120]
        print(f"  {mark} {arg} ({kind}) {about}")
    print()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("tools", nargs="*", help="slugs such as GMAIL_SEND_EMAIL or FASTMAIL_READ_EMAIL")
    parser.add_argument("--list", metavar="TOOLKIT", help="list a Composio toolkit's slugs, or 'fastmail'")
    parser.add_argument("--config", default=".dev/android-server.toml")
    parser.add_argument("--json", action="store_true", help="print the raw schema")
    args = parser.parse_args()
    with open(args.config, "rb") as file:
        config = tomllib.load(file)

    if args.list == "fastmail":
        print("\n".join(sorted(fastmail_tools(config["database_url"]))))
        return
    if args.list:
        listed = composio(config["composio_key"], f"tools?toolkit_slug={args.list}&limit=500")
        print("\n".join(sorted(tool["slug"] for tool in listed.get("items", []))))
        return

    fastmail = None
    for tool in args.tools:
        if tool.upper().startswith("FASTMAIL_"):
            if fastmail is None:
                fastmail = fastmail_tools(config["database_url"])
            found = fastmail.get(tool.upper().removeprefix("FASTMAIL_").lower())
            if not found:
                print(f"## {tool}: not in the cached Fastmail tools\n")
                continue
            schema, description = found["input_schema"], found.get("description", "")
        else:
            found = composio(config["composio_key"], f"tools/{tool.upper()}")
            if "error" in found:
                print(f"## {tool}: {found['error']}\n")
                continue
            schema, description = found["input_parameters"], found.get("description", "")
        if args.json:
            json.dump({"name": tool, "input": schema}, sys.stdout, indent=1)
            print()
        else:
            show(tool.upper(), schema, description)


if __name__ == "__main__":
    main()
