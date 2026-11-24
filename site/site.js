// The generated values (from data.js), scroll reveal and the hero speck field, as on
// midasvo.nl/ephyra/. Without data.js (not committed; pages.yaml writes it) or without
// JavaScript the page is complete, with links where the versions would be.
(function () {
  "use strict";

  var repo = "https://github.com/midasvo/libmpv-android";
  var names = { ffmpeg: "FFmpeg" };

  function each(selector, fn) {
    Array.prototype.forEach.call(document.querySelectorAll(selector), fn);
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  }

  function fill(data) {
    var byName = {};
    data.components.forEach(function (c) {
      byName[c.name] = c;
    });
    var values = {
      release: data.release,
      "release-mpv": byName.mpv ? byName.mpv.release : "",
      "release-ffmpeg": byName.ffmpeg ? byName.ffmpeg.release : "",
      ndk: data.ndk.main,
      commit: data.commit.slice(0, 7)
    };
    Object.keys(values).forEach(function (k) {
      each('[data-v="' + k + '"]', function (e) {
        e.textContent = values[k];
      });
    });
    each('[data-v="commit-link"]', function (e) {
      e.href = repo + "/tree/" + data.commit;
    });
    each('[data-v="release-link"]', function (e) {
      e.href = repo + "/releases/tag/v" + data.release;
    });

    each('[data-v="components"]', function (tbody) {
      data.components.forEach(function (c) {
        var tr = el("tr");
        var th = el("th", "", names[c.name] || c.name);
        th.scope = "row";
        tr.appendChild(th);
        tr.appendChild(version(c.release, c.releasePatches, false));
        tr.appendChild(version(c.main, c.mainPatches, c.main !== c.release));
        tr.appendChild(el("td", "", c.licence));
        tbody.appendChild(tr);
      });
    });

    each('[data-v="others"]', function (p) {
      p.textContent = "Also in the AAR: " + data.others.map(function (o) {
        return o.name + " (" + o.licence + ")";
      }).join(", ") + ".";
    });

    // The hand-written release text describes one release; hide it once a newer one is out
    each("[data-written-for]", function (e) {
      if (e.getAttribute("data-written-for") !== data.release) {
        e.hidden = true;
        each("[data-text-stale]", function (p) {
          p.hidden = false;
        });
      }
    });

    each("[data-with-data]", function (e) {
      e.hidden = false;
    });
    each("[data-without-data]", function (e) {
      e.hidden = true;
    });
  }

  function version(v, patches, changed) {
    var td = el("td", changed ? "v changed" : "v", v || "–");
    if (patches && patches.length) {
      var s = el("span", "patched", "+ patch");
      s.title = patches.join("\n");
      td.appendChild(document.createTextNode(" "));
      td.appendChild(s);
    }
    return td;
  }

  var data = window.SITE_DATA;
  if (data && data.release && data.components) fill(data);

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
