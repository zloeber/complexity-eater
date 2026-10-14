(function () {
  try {
    var theme = localStorage.getItem("ce-theme");
    if (theme === "light" || theme === "dark") {
      document.documentElement.setAttribute("data-theme", theme);
    }
  } catch (error) {
    /* The theme stays on the system preference when storage is blocked. */
  }
})();
