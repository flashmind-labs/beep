import { el } from "../ui/dom.ts";
import { IC } from "../ui/icons.ts";

export function filePicker(attach: (files: File[]) => unknown) {
  const input = el("input");
  input.type = "file";
  input.multiple = true;
  input.hidden = true;
  const button = el("button", "dbtn quiet");
  button.type = "button";
  button.title = "Attach files";
  button.setAttribute("aria-label", "Attach files");
  button.innerHTML = IC.plus;
  button.addEventListener("click", () => input.click());
  input.addEventListener("change", () => {
    const files = [...(input.files ?? [])];
    input.value = "";
    if (files.length) void attach(files);
  });
  return { button, input };
}
