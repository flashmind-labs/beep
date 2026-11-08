// The title stays editable while session updates arrive in the background.
export function bindTitleEdit(title: HTMLElement, options: { value: () => string; save: (value: string) => Promise<void> }) {
  title.tabIndex = 0;
  title.setAttribute("role", "button");
  title.setAttribute("aria-label", "Rename session");
  title.title = "Double-click to rename (Enter)";
  const edit = () => {
    if (title.querySelector("input")) return;
    const input = document.createElement("input");
    input.className = "agtitle-input";
    input.value = options.value();
    input.maxLength = 200;
    input.setAttribute("aria-label", "Session title");
    input.title = "Enter to save, Escape to cancel. Clear to restore the agent title.";
    let saving = false, done = false;
    const error = document.createElement("small");
    error.className = "agtitle-error";
    error.setAttribute("role", "alert");
    const finish = () => {
      done = true;
      error.remove();
      title.textContent = options.value();
    };
    const save = async () => {
      if (done || saving) return;
      if (!title.isConnected) return;
      if (input.value.trim() === options.value()) { finish(); return; }
      saving = true;
      input.readOnly = true;
      title.setAttribute("aria-busy", "true");
      error.remove();
      try {
        await options.save(input.value.trim());
        finish();
      } catch (e) {
        if (title.isConnected) {
          error.textContent = `Could not save title: ${e instanceof Error ? e.message : String(e)}`;
          title.after(error);
          input.setAttribute("aria-invalid", "true");
          input.focus();
        }
      } finally {
        saving = false;
        input.readOnly = false;
        title.removeAttribute("aria-busy");
      }
    };
    input.addEventListener("blur", () => void save());
    input.addEventListener("keydown", e => {
      e.stopPropagation();
      if (e.isComposing) return;
      if (e.key === "Escape") {
        e.preventDefault();
        if (!saving) { finish(); title.focus(); }
      } else if (e.key === "Enter") {
        e.preventDefault();
        void save().then(() => { if (done && title.isConnected) title.focus(); });
      }
    });
    title.replaceChildren(input);
    input.focus();
    input.select();
  };
  title.addEventListener("dblclick", edit);
  title.addEventListener("keydown", e => {
    if (e.target === title && e.key === "Enter") { e.preventDefault(); e.stopPropagation(); edit(); }
  });
}
