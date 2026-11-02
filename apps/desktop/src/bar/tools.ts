/// Tool steps shown while a run works; older ones fold into "+N tools".
export const VISIBLE_STEPS = 3;

/// Tools that get no step: what they do is already on screen, or is bookkeeping.
export const QUIET_TOOLS = new Set(["suggest_replies", "show_products", "show_cards", "show_chart", "notify_user", "task_progress"]);

/// What a tool is doing, in words, the same as the phone's toolLabel.
/// Unknown tools fall back to their name: "GMAIL_SEND_EMAIL" becomes "Gmail: send email".
export const TOOL_LABELS: Record<string, string> = {
  web_search: "Searching the web",
  decision: "Weighing the options",
  get_location: "Checking your location",
  get_time: "Checking the time",
  open_app: "Opening an app",
  set_alarm: "Setting an alarm",
  open_browser: "Opening a page",
  browser: "Browsing",
  mcp_search: "Finding the right app tool",
  mcp: "Using a connected app",
  open_maps: "Looking up a place",
  query_contacts: "Looking up contacts",
  suggest_replies: "Suggesting replies",
  send_file: "Sending a file",
  attach: "Looking at a file",
  show_products: "Showing products",
  show_cards: "Showing recommendations",
  show_chart: "Drawing a chart",
  show_stock_chart: "Looking up stocks",
  task_progress: "Updating progress",
  memory_store: "Remembering",
  memory_recall: "Recalling memories",
  memory_update: "Updating a memory",
  memory_forget: "Forgetting a memory",
  cron_create: "Setting a reminder",
  schedule_once: "Setting a reminder",
  cron_edit: "Changing a reminder",
  cron_list: "Checking reminders",
  cron_delete: "Deleting a reminder",
  cron_history: "Checking past reminders",
  habit_create: "Creating a habit",
  habit_log: "Logging a habit",
  habit_status: "Checking habits",
  habit_delete: "Deleting a habit",
  // One tool per family with an action argument. While it runs the server's
  // own description names the action; a saved step carries a label read from
  // its result, and these cover the rest.
  memory: "Checking memories",
  skill: "Loading a skill",
  habit: "Checking habits",
  background: "Checking background work",
  reminder: "Setting a reminder",
  node: "Using your machines",
  // Commands stay out of the chat: every one reads the same.
  exec: "Running code",
  process: "Running code",
  desktop: "Using the Mac",
  code_agent: "Using a coding agent",
  get_route: "Planning a route",
  send_image: "Sending pictures",
  chat_search: "Searching this chat",
  chat_read: "Reading this chat",
  place_save: "Saving a place",
  place_list: "Checking your places",
  place_delete: "Forgetting a place",
  place_routine: "Setting up a place routine",
  event_topics: "Checking which updates you get",
  hold_events: "Holding updates for later",
  recent_events: "Checking recent updates",
  execute_code: "Running code",
  download_file: "Downloading a file",
  web_scrape: "Reading a page",
  web_extract: "Reading a page",
  web_map: "Mapping a site",
  web_crawl: "Reading a site",
  web_search_read: "Reading search results",
  start_research: "Starting research",
  research_more: "Asking the research for more",
  stop_research: "Stopping the research",
  end_routine: "Ending this routine",
  request_grant: "Asking to allow a routine",
  // Names from before the fold, still in older chats.
  memory_history: "Checking how a memory changed",
  spawn_agent: "Starting an agent",
  message_agent: "Messaging an agent",
  run_in_background: "Moving work to the background",
  jobs: "Checking background work",
  wait_job: "Waiting for background work",
  cancel_job: "Stopping background work",
};
// Tools whose own description says more than their name, such as "Reading
// lease.pdf" or which action of a folded tool runs.
export const DESCRIBED = new Set([
  "memory", "skill", "habit", "background", "reminder", "node",
  "desktop", "code_agent", "send_file", "exec", "attach",
]);
/// A finished step's label in the past tense, as on the phone: "Recalling
/// memories" becomes "Recalled memories". Unknown verbs stay as they are.
export const PAST: Record<string, string> = {
  searching: "Searched", reading: "Read", checking: "Checked", looking: "Looked", fetching: "Fetched",
  opening: "Opened", saving: "Saved", finding: "Found", getting: "Got", creating: "Created",
  updating: "Updated", deleting: "Deleted", sending: "Sent", setting: "Set", using: "Used",
  remembering: "Remembered", loading: "Loaded", browsing: "Browsed", scheduling: "Scheduled",
  running: "Ran", weighing: "Weighed", recalling: "Recalled", forgetting: "Forgot", changing: "Changed",
  logging: "Logged", suggesting: "Suggested", showing: "Showed", starting: "Started", messaging: "Messaged",
  moving: "Moved", waiting: "Waited", stopping: "Stopped", holding: "Held", planning: "Planned",
  drawing: "Drew", downloading: "Downloaded", mapping: "Mapped", asking: "Asked", ending: "Ended",
  following: "Followed", unfollowing: "Unfollowed", taking: "Took", answering: "Answered",
  clicking: "Clicked", typing: "Typed", pressing: "Pressed", scrolling: "Scrolled", writing: "Wrote",
  linking: "Linked", copying: "Copied", typesetting: "Typeset",
};
export function doneLabel(label: string) {
  const text = label.replace(/[.\u2026\s]+$/, "");
  const head = text.split(" ", 1)[0];
  const past = PAST[head.toLowerCase()];
  return past ? past + text.slice(head.length) : text;
}
export const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
export function toolLabel(name: string) {
  if (TOOL_LABELS[name]) return TOOL_LABELS[name];
  const words = name.split(/[_.-]/).filter(w => w.trim()).map(w => w.toLowerCase());
  if (!words.length) return "";
  // Connected app tools are named APP_ACTION.
  if (name === name.toUpperCase() && words.length > 1) return cap(words[0]) + ": " + words.slice(1).join(" ");
  return cap(words.join(" "));
}

