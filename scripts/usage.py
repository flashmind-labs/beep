"""Prints model spend per agent and the costliest user-days, to size a cap.

    just usage        # the last 14 days
    just usage 30

Reads `token_usage` from the database in the server config given with
--config. Agent "-" is spend outside an agent's turn: Jev, memory, titles,
follow-ups, routines without an agent, and turns while agents are off.
"""

import argparse
import tomllib

import psycopg

PER_AGENT = """
select agent, count(distinct user_id), count(distinct (user_id, day)), sum(calls),
       sum(prompt_tokens), sum(cached_tokens), sum(completion_tokens), sum(cost)
from token_usage where day > current_date - %(days)s
group by agent order by sum(cost) desc
"""

# Per user and day, as a cap would count it.
USER_DAYS = """
with days as (
    select user_id, day, agent, sum(cost) as cost
    from token_usage where day > current_date - %(days)s
    group by user_id, day, agent
)
select agent, percentile_cont(0.5) within group (order by cost),
       percentile_cont(0.9) within group (order by cost), max(cost)
from days group by agent order by max(cost) desc
"""

TOP = """
select day, user_id, sum(cost),
       string_agg(coalesce(nullif(agent, ''), '-') || ' ' || to_char(cost, 'FM990.0000'), ', '
                  order by cost desc)
from (select day, user_id, agent, sum(cost) as cost from token_usage
      where day > current_date - %(days)s group by day, user_id, agent) t
group by day, user_id order by sum(cost) desc limit 10
"""


def agent(name):
    return name or "-"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("days", nargs="?", type=int, default=14)
    parser.add_argument("--config", default=".dev/android-server.toml")
    args = parser.parse_args()
    with open(args.config, "rb") as file:
        url = tomllib.load(file)["database_url"]
    query = {"days": args.days}
    with psycopg.connect(url) as db:
        print(f"Last {args.days} days, USD at list prices\n")
        print(f"{'agent':16} {'users':>5} {'user-days':>9} {'calls':>7} {'prompt':>11} {'cached':>11} {'output':>9} {'cost':>9}")
        for row in db.execute(PER_AGENT, query):
            name, users, user_days, calls, prompt, cached, output, cost = row
            print(f"{agent(name):16} {users:5} {user_days:9} {calls:7} {prompt:11} {cached:11} {output:9} {cost:9.4f}")

        print(f"\nCost per user-day\n{'agent':16} {'median':>9} {'p90':>9} {'max':>9}")
        for name, median, p90, top in db.execute(USER_DAYS, query):
            print(f"{agent(name):16} {median:9.4f} {p90:9.4f} {top:9.4f}")

        print("\nCostliest user-days")
        for day, user, cost, split in db.execute(TOP, query):
            print(f"{day} {user[:12]:12} {cost:8.4f}  {split}")


if __name__ == "__main__":
    main()
