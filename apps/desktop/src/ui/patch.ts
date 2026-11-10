/// Makes `target` hold `html`, changing only what differs. An element of the same kind in the same
/// place stays the same element, with its attributes and text updated in place, so a spinner keeps
/// turning and a scroll position or a selection stays where it is.
export function patchHtml(target: Element, html: string) {
  const t = document.createElement("template");
  t.innerHTML = html;
  patchNodes(target, t.content);
}

function patchNodes(to: ParentNode, from: ParentNode) {
  const have = [...to.childNodes], want = [...from.childNodes];
  want.forEach((next, i) => {
    const old = have[i];
    if (!old) return void to.append(next);

    if (old instanceof Element && next instanceof Element && old.tagName === next.tagName) {
      for (const a of [...old.attributes]) if (!next.hasAttribute(a.name)) old.removeAttribute(a.name);
      for (const a of [...next.attributes]) if (old.getAttribute(a.name) !== a.value) old.setAttribute(a.name, a.value);
      return patchNodes(old, next);
    }

    if (old.nodeType === next.nodeType && !(old instanceof Element)) {
      if (old.nodeValue !== next.nodeValue) old.nodeValue = next.nodeValue;
      return;
    }

    old.replaceWith(next);
  });
  for (const gone of have.slice(want.length)) gone.remove();
}
