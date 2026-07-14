document.documentElement.classList.add("js");

const revealElements = document.querySelectorAll(".reveal");

if ("IntersectionObserver" in window) {
  const observer = new IntersectionObserver(
    (entries, revealObserver) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;

        entry.target.classList.add("is-visible");
        revealObserver.unobserve(entry.target);
      }
    },
    {
      rootMargin: "0px 0px -8%",
      threshold: 0.12,
    },
  );

  revealElements.forEach((element) => observer.observe(element));
} else {
  revealElements.forEach((element) => element.classList.add("is-visible"));
}
