(() => {
  const toc = document.querySelector("[data-article-toc]");
  const article = document.querySelector("[data-article-body]");
  const list = toc?.querySelector("[data-article-toc-list]");
  const scroller = toc?.querySelector("[data-article-toc-scroll]");
  if (!toc || !article || !list || !scroller) return;

  const headings = [...article.querySelectorAll("h1, h2, h3, h4, h5, h6")];
  if (!headings.length) return;

  const usedIds = new Set([...document.querySelectorAll("[id]")].map((element) => element.id));
  const links = [];

  const uniqueId = (heading, index) => {
    const existing = heading.id || heading.querySelector("[id]")?.id;
    if (existing) return existing;
    const base = (heading.textContent || `section-${index + 1}`)
      .trim()
      .toLowerCase()
      .normalize("NFKC")
      .replace(/[^\p{L}\p{N}\s-]/gu, "")
      .replace(/\s+/g, "-") || `section-${index + 1}`;
    let id = `toc-${base}`;
    let suffix = 2;
    while (usedIds.has(id)) id = `toc-${base}-${suffix++}`;
    heading.id = id;
    usedIds.add(id);
    return id;
  };

  headings.forEach((heading, index) => {
    const id = uniqueId(heading, index);
    const item = document.createElement("li");
    item.className = `article-toc__item article-toc__item--depth-${heading.tagName.slice(1)}`;
    const link = document.createElement("a");
    link.href = `#${encodeURIComponent(id)}`;
    link.textContent = heading.textContent?.trim() || `Section ${index + 1}`;
    link.addEventListener("click", (event) => {
      event.preventDefault();
      heading.scrollIntoView({ behavior: "smooth", block: "start" });
      history.pushState(null, "", `#${encodeURIComponent(id)}`);
    });
    item.append(link);
    list.append(item);
    links.push(link);
  });

  toc.hidden = false;

  let activeIndex = -1;
  let scheduled = false;
  const updateActive = () => {
    scheduled = false;
    const header = document.querySelector(".earth-header");
    const threshold = (header?.getBoundingClientRect().height || 54) + 24;
    let nextIndex = 0;
    headings.forEach((heading, index) => {
      if (heading.getBoundingClientRect().top <= threshold) nextIndex = index;
    });
    if (nextIndex === activeIndex) return;
    activeIndex = nextIndex;
    links.forEach((link, index) => {
      const active = index === activeIndex;
      link.classList.toggle("is-active", active);
      if (active) link.setAttribute("aria-current", "location");
      else link.removeAttribute("aria-current");
    });
    const activeLink = links[activeIndex];
    if (!activeLink) return;
    const top = activeLink.offsetTop;
    const bottom = top + activeLink.offsetHeight;
    if (top < scroller.scrollTop) scroller.scrollTo({ top, behavior: "smooth" });
    else if (bottom > scroller.scrollTop + scroller.clientHeight) {
      scroller.scrollTo({ top: bottom - scroller.clientHeight, behavior: "smooth" });
    }
  };

  const scheduleUpdate = () => {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(updateActive);
  };
  window.addEventListener("scroll", scheduleUpdate, { passive: true });
  window.addEventListener("resize", scheduleUpdate, { passive: true });
  updateActive();
})();
