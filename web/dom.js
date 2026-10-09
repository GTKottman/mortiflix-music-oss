// The studio's tiny DOM builder, shared by every page. Text is always inserted as text, never as HTML.

export function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'style') for (const [prop, val] of Object.entries(v)) { if (prop.startsWith('--')) el.style.setProperty(prop, val); else el.style[prop] = val; }
    else if (k === 'value') el.value = v;
    else if (k === 'checked') el.checked = Boolean(v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const kid of kids.flat(Infinity)) {
    if (kid === null || kid === undefined || kid === false) continue;
    el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

// replaceChildren() would print null as "null": skipped sections are dropped here.
export function fill(el, ...kids) {
  el.replaceChildren(...kids.flat(Infinity).filter((k) => k !== null && k !== undefined && k !== false));
}

// append() would print null as "null" too.
export function add(el, ...kids) {
  el.append(...kids.flat(Infinity).filter((k) => k !== null && k !== undefined && k !== false));
}
