(() => {
  const carousel = document.querySelector(".showcase-carousel");
  const track = carousel.querySelector(".showcase-track");
  const slides = Array.from(carousel.querySelectorAll(".showcase-slide"));
  const dots = Array.from(carousel.querySelectorAll("[data-slide]"));
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  let current = 0;
  let visible = false;
  let timer;

  function show(index) {
    current = index;
    track.style.transform = `translateX(-${current * 100}%)`;
    slides.forEach((slide, i) => {
      slide.setAttribute("aria-hidden", String(i !== current));
      slide.inert = i !== current;
      dots[i].setAttribute("aria-pressed", String(i === current));
    });
  }

  function schedule() {
    clearInterval(timer);
    if (reducedMotion.matches || !visible || document.hidden) return;
    timer = setInterval(() => show((current + 1) % slides.length), 3500);
  }

  dots.forEach((dot, i) => dot.addEventListener("click", () => {
    show(i);
    schedule();
  }));
  document.addEventListener("visibilitychange", schedule);
  reducedMotion.addEventListener("change", schedule);
  new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    schedule();
  }, { threshold: 0.1 }).observe(carousel);

  carousel.classList.add("is-ready");
  carousel.querySelector(".carousel-controls").hidden = false;
  show(0);
  schedule();
})();
