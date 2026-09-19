(() => {
  const storageKey = "tambat.site.language";
  const selector = document.querySelector(".language-switcher");
  const buttons = selector.querySelectorAll("[data-language]");
  const languages = Array.from(buttons, button => button.dataset.language);
  const summary = selector.querySelector("summary");
  const translations = [];

  // Indonesian stays in the HTML so the page also works without JavaScript.
  for (const attribute of [null, "alt", "aria-label", "content"]) {
    const marker = attribute ? `data-en-${attribute}` : "data-en";
    for (const element of document.querySelectorAll(`[${marker}]`)) {
      const entry = {
        element,
        attribute,
        id: attribute ? element.getAttribute(attribute) : element.textContent,
      };
      for (const language of languages.filter(language => language !== "id")) {
        entry[language] = element.getAttribute(marker.replace("data-en", `data-${language}`));
      }
      translations.push(entry);
    }
  }

  function setLanguage(language) {
    for (const entry of translations) {
      if (entry.attribute) entry.element.setAttribute(entry.attribute, entry[language]);
      else entry.element.textContent = entry[language];
    }
    document.documentElement.lang = language;
    for (const button of buttons) {
      button.setAttribute("aria-pressed", String(button.dataset.language === language));
      if (button.dataset.language === language) {
        selector.querySelector(".language-current-flag").replaceChildren(button.querySelector("svg").cloneNode(true));
        selector.querySelector(".language-current-name").textContent = button.querySelector("span").textContent;
        summary.lang = language;
      }
    }
  }

  let saved;
  try {
    saved = localStorage.getItem(storageKey);
  } catch {
    // Browsers can block storage; switching languages must still work.
  }
  const preferred = (navigator.languages || [navigator.language || "id"])
    .map(language => language.toLowerCase().split("-")[0])
    .find(language => languages.includes(language));
  setLanguage(languages.includes(saved) ? saved : preferred || "en");

  selector.hidden = false;
  for (const button of buttons) {
    button.addEventListener("click", () => {
      const language = button.dataset.language;
      setLanguage(language);
      selector.open = false;
      summary.focus();
      try {
        localStorage.setItem(storageKey, language);
      } catch {
        // The selection still applies to the current page if storage is blocked.
      }
    });
  }
  document.addEventListener("click", event => {
    if (!selector.contains(event.target)) selector.open = false;
  });
  selector.addEventListener("keydown", event => {
    if (event.key === "Escape") {
      selector.open = false;
      summary.focus();
    }
  });
})();
