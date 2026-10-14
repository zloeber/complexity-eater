(function () {
  var root = document.documentElement;
  var toggle = document.getElementById("theme-toggle");
  var navButton = document.querySelector(".nav-toggle");
  var header = document.querySelector(".site-header");

  function currentTheme() {
    var explicit = root.getAttribute("data-theme");
    if (explicit === "light" || explicit === "dark") {
      return explicit;
    }
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  function labelTheme() {
    if (!toggle) {
      return;
    }
    var theme = currentTheme();
    toggle.setAttribute("aria-pressed", theme === "dark" ? "true" : "false");
    toggle.textContent = theme === "dark" ? "Use light theme" : "Use dark theme";
  }

  if (toggle) {
    toggle.addEventListener("click", function () {
      var next = currentTheme() === "dark" ? "light" : "dark";
      root.setAttribute("data-theme", next);
      try {
        localStorage.setItem("ce-theme", next);
      } catch (error) {
        /* The choice still applies for this page view. */
      }
      labelTheme();
    });
  }
  labelTheme();

  if (navButton && header) {
    navButton.addEventListener("click", function () {
      var open = header.classList.toggle("is-open");
      navButton.setAttribute("aria-expanded", open ? "true" : "false");
    });
    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape" && header.classList.contains("is-open")) {
        header.classList.remove("is-open");
        navButton.setAttribute("aria-expanded", "false");
        navButton.focus();
      }
    });
  }

  document.querySelectorAll("[data-copy]").forEach(function (button) {
    button.addEventListener("click", function () {
      var target = document.getElementById(button.getAttribute("data-copy"));
      if (!target) {
        return;
      }
      var text = target.textContent || "";
      if (!navigator.clipboard) {
        button.textContent = "Select the command";
        return;
      }
      var original = button.textContent;
      navigator.clipboard.writeText(text).then(
        function () {
          button.textContent = "Copied";
          window.setTimeout(function () {
            button.textContent = original;
          }, 2000);
        },
        function () {
          button.textContent = "Copy failed";
        }
      );
    });
  });
})();
