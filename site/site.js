// Scroll reveal and the hero speck field, as on midasvo.nl/ephyra/.
// The page is complete without this file; it only adds motion.
(function () {
  "use strict";

  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  if ("IntersectionObserver" in window && !reduced) {
    document.documentElement.classList.add("js");
    var io = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (e) {
          if (e.isIntersecting) {
            e.target.classList.add("visible");
            io.unobserve(e.target);
          }
        });
      },
      { rootMargin: "0px 0px -10% 0px" }
    );
    document.querySelectorAll(".reveal").forEach(function (el) {
      io.observe(el);
    });
  }

  var field = document.querySelector(".specks");
  if (!field) return;
  var count = Math.min(32, Math.round(window.innerWidth / 36));
  var frag = document.createDocumentFragment();
  for (var i = 0; i < count; i++) {
    var s = document.createElement("span");
    s.style.left = Math.random() * 100 + "%";
    s.style.top = 20 + Math.random() * 80 + "%";
    s.style.setProperty("--o", (0.15 + Math.random() * 0.25).toFixed(2));
    s.style.setProperty("--d", 30 + Math.random() * 30 + "s");
    s.style.setProperty("--delay", -Math.random() * 60 + "s");
    s.style.setProperty("--dx", (Math.random() * 40 - 20).toFixed(0) + "px");
    var size = 2 + Math.random() * 2;
    s.style.width = s.style.height = size.toFixed(1) + "px";
    frag.appendChild(s);
  }
  field.appendChild(frag);
})();
