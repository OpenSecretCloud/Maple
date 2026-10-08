"use strict";
// Only native callAsyncJavaScript arguments enter this function. There are no
// native message handlers, transcript DOM, storage writes, links, or fetches.
window.mapleEquation = async function(source, display) {
  const element = document.getElementById("equation");
  element.replaceChildren();
  if (typeof source !== "string" || new TextEncoder().encode(source).length > 16384) return null;
  try {
    const html = katex.renderToString(source, {
      displayMode: display === true, output: "html", throwOnError: true,
      trust: false, strict: "error", maxExpand: 1000, maxSize: 16,
      macros: Object.create(null), globalGroup: false
    });
    if (html.length > 262144) return null;
    element.innerHTML = html;
    if (element.querySelectorAll("*").length > 4096) { element.replaceChildren(); return null; }
    // Font readiness is bounded by native timeout; packaged woff2 is the only
    // permitted font source. No user-controlled DOM attribute is trusted.
    // Flush style/layout first: otherwise ready may be an already-resolved
    // promise before this formula has requested its packaged font faces.
    element.getBoundingClientRect();
    await document.fonts.ready;
    const box = element.getBoundingClientRect();
    const baseline = document.getElementById("baseline").getBoundingClientRect().top;
    if (![box.width, box.height, baseline].every(Number.isFinite) || box.width < 1 || box.height < 1 || box.width > 4096 || box.height > 2048) {
      element.replaceChildren(); return null;
    }
    return {width: Math.ceil(box.width), height: Math.ceil(box.height), descent: Math.max(0, box.bottom - baseline)};
  } catch (_) { element.replaceChildren(); return null; }
};
