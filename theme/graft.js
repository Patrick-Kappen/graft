(() => {
  const root = typeof path_to_root === "string" ? path_to_root : "";
  const menuTitle = document.querySelector(".menu-title");
  const sidebar = document.getElementById("mdbook-sidebar");
  const rightButtons = document.querySelector(".right-buttons");
  const themeColor = document.querySelector('meta[name="theme-color"]');

  if (themeColor) themeColor.setAttribute("content", "#080b0f");

  if (menuTitle) {
    const homeLink = document.createElement("a");
    homeLink.className = "graft-menu-brand";
    homeLink.href = `${root}index.html`;
    homeLink.innerHTML = '<span aria-hidden="true">G</span><strong>Graft</strong><small>Documentation</small>';
    homeLink.setAttribute("aria-label", "Graft documentation home");
    menuTitle.replaceChildren(homeLink);
  }

  if (sidebar) {
    const sidebarBrand = document.createElement("div");
    sidebarBrand.className = "graft-sidebar-brand";
    sidebarBrand.innerHTML = `
      <a href="${root}index.html" aria-label="Graft documentation home">
        <span class="graft-brand-mark" aria-hidden="true">G</span>
        <span><strong>Graft</strong><small>Documentation</small></span>
      </a>
      <p>TOML intent. Nix-built rootfs. Systemd-native runtime.</p>
    `;
    sidebar.prepend(sidebarBrand);
  }

  if (rightButtons) {
    const portfolioLink = document.createElement("a");
    portfolioLink.className = "graft-header-link";
    portfolioLink.href = "https://patrick.kappen.io";
    portfolioLink.textContent = "Patrick Kappen";
    portfolioLink.setAttribute("aria-label", "Visit Patrick Kappen's portfolio");

    const projectLink = document.createElement("a");
    projectLink.className = "graft-header-link graft-project-link";
    projectLink.href = "/";
    projectLink.textContent = "Graft home";

    rightButtons.prepend(portfolioLink);
    rightButtons.prepend(projectLink);
  }
})();
