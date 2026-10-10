(() => {
  "use strict";

  const T = window.__TAURI__;

  const bridge = {
    getNodes() {
      return T.core.invoke("get_nodes");
    },
    refreshNodes() {
      return T.core.invoke("refresh_nodes");
    },
    startTest(node, mode) {
      return T.core.invoke("start_test", { node, mode });
    },
    loadConfig() {
      return T.core.invoke("load_config");
    },
    saveConfig(cfg) {
      return T.core.invoke("save_config", { cfg });
    },
    on(event, cb) {
      T.event.listen(event, (e) => cb(e.payload));
    },
  };

  const TICKS = [0, 4, 20, 40, 80, 200, 600, 1200, 2000];
  const START = Math.PI * 0.75;
  const SWEEP = Math.PI * 1.5;

  function valueToFrac(v) {
    if (!(v > 0)) return 0;
    const last = TICKS[TICKS.length - 1];
    if (v >= last) return 1;
    for (let i = 1; i < TICKS.length; i++) {
      if (v <= TICKS[i]) {
        const lo = TICKS[i - 1];
        const hi = TICKS[i];
        const t = (v - lo) / (hi - lo);
        return (i - 1 + t) / (TICKS.length - 1);
      }
    }
    return 1;
  }

  const S = {
    nodes: [],
    filtered: [],
    nodesError: "",
    phase: -1,
    running: false,
    target: 0,
    disp: 0,
    ping: null,
  };

  const $ = (id) => document.getElementById(id);
  const el = {
    gauge: $("gaugeCanvas"),
    dial: $("dialBox"),
    gvNum: $("gvNum"), gvUnit: $("gvUnit"), gvIcon: $("gvIcon"),
    mPing: $("mPing"), mDown: $("mDown"), mUp: $("mUp"),
    mPingBox: $("mPingBox"), mDownBox: $("mDownBox"), mUpBox: $("mUpBox"),
    srvName: $("srvName"), srvSub: $("srvSub"), ispName: $("ispName"),
    serverRow: $("serverRow"),
    moreBtn: $("moreBtn"),
    selProv: $("selProv"), selOper: $("selOper"), selNode: $("selNode"),
    nodeSheet: $("nodeSheet"), menuSheet: $("menuSheet"), sheetMask: $("sheetMask"),
    sheetCancel: $("sheetCancel"), sheetConfirm: $("sheetConfirm"),
    menuRefresh: $("menuRefresh"), menuClose: $("menuClose"),
    menuPingOnly: $("menuPingOnly"), menuDownOnly: $("menuDownOnly"),
    menuUpOnly: $("menuUpOnly"),
    toast: $("toast"),
  };

  const ICON = {
    ping: '<svg viewBox="0 0 16 16" width="15" height="15"><circle cx="8" cy="8" r="7" fill="none" stroke="currentColor" stroke-width="1.2"/><circle cx="8" cy="8" r="2.1" fill="currentColor"/></svg>',
    down: '<svg viewBox="0 0 16 16" width="15" height="15"><circle cx="8" cy="8" r="7" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M8 4.6 V11.1 M5.1 8.2 L8 11.1 L10.9 8.2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    up: '<svg viewBox="0 0 16 16" width="15" height="15"><circle cx="8" cy="8" r="7" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M8 11.4 V4.9 M5.1 7.8 L8 4.9 L10.9 7.8" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  };

  const PLACEHOLDER = "\u2026";

  function fitCanvas(cv) {
    const dpr = window.devicePixelRatio || 1;
    const r = cv.getBoundingClientRect();
    const w = Math.max(1, Math.round(r.width));
    const h = Math.max(1, Math.round(r.height));
    if (cv.width !== w * dpr || cv.height !== h * dpr) {
      cv.width = w * dpr;
      cv.height = h * dpr;
    }
    const ctx = cv.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    return { ctx, w, h };
  }

  let toastTimer = 0;
  function toast(msg, isError) {
    el.toast.textContent = msg;
    el.toast.classList.toggle("error", !!isError);
    el.toast.classList.add("show");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => el.toast.classList.remove("show"), 4200);
  }

  function drawGauge() {
    const { ctx, w, h } = fitCanvas(el.gauge);
    const cx = w / 2;
    const cy = h / 2;
    const Rout = Math.min(w, h) / 2;
    const lw = Rout * 0.167;
    const Rc = Rout - lw / 2;

    ctx.clearRect(0, 0, w, h);

    ctx.beginPath();
    ctx.arc(cx, cy, Rc, START, START + SWEEP);
    ctx.strokeStyle = "#222f4f";
    ctx.lineWidth = lw;
    ctx.lineCap = "butt";
    ctx.stroke();

    const frac = valueToFrac(S.disp);

    if (frac > 0.0015) {
      const end = START + SWEEP * frac;

      let stroke;
      if (typeof ctx.createConicGradient === "function") {
        const g = ctx.createConicGradient(START, cx, cy);
        g.addColorStop(0.0, "#2fc9f7");
        g.addColorStop(0.075, "#3dd6fa");
        g.addColorStop(0.26, "#67f9e7");
        g.addColorStop(0.45, "#73ffc0");
        g.addColorStop(0.75, "#7dffb4");
        g.addColorStop(1.0, "#7dffb4");
        stroke = g;
      } else {
        const g = ctx.createLinearGradient(cx - Rout, cy + Rout, cx + Rout, cy - Rout);
        g.addColorStop(0, "#3dd6fa");
        g.addColorStop(0.55, "#67f9e7");
        g.addColorStop(1, "#73ffc0");
        stroke = g;
      }
      ctx.beginPath();
      ctx.arc(cx, cy, Rc, START, end);
      ctx.strokeStyle = stroke;
      ctx.lineWidth = lw;
      ctx.stroke();

    }

    const Rin = Rout - lw;
    const fontSize = Rout * 0.0873;
    const tickPadding = Rout * 0.042;
    const hh = fontSize * 0.36;
    ctx.font = `${fontSize}px "Segoe UI", "Microsoft YaHei", sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillStyle = "rgba(255,255,255,0.96)";
    for (let i = 0; i < TICKS.length; i++) {
      const a = START + (SWEEP * i) / (TICKS.length - 1);
      const str = String(TICKS[i]);
      const hw = ctx.measureText(str).width / 2;
      const ext = Math.abs(Math.cos(a)) * hw + Math.abs(Math.sin(a)) * hh;
      const r = Rin - tickPadding - ext;
      ctx.fillText(str, cx + Math.cos(a) * r, cy + Math.sin(a) * r);
    }

    if (frac > 0.0015) {
      const ang = START + SWEEP * frac;
      const r0 = Rout * 0.105;
      const r1 = Rout * 0.615;
      const shoulder = Rout * 0.075;
      const halfTip = Rout * 0.032;
      const halfRoot = Rout * 0.007;

      ctx.save();
      ctx.translate(cx, cy);
      ctx.rotate(ang);
      ctx.beginPath();
      ctx.moveTo(r1, 0);
      ctx.lineTo(r1 - shoulder, -halfTip);
      ctx.lineTo(r0 + shoulder * 0.5, -halfRoot);
      ctx.lineTo(r0, 0);
      ctx.lineTo(r0 + shoulder * 0.5, halfRoot);
      ctx.lineTo(r1 - shoulder, halfTip);
      ctx.closePath();

      const needle = ctx.createLinearGradient(r0, 0, r1, 0);
      needle.addColorStop(0, "#111a2b");
      needle.addColorStop(0.45, "#465371");
      needle.addColorStop(0.8, "#a7b0c4");
      needle.addColorStop(1, "#f5f6f7");
      ctx.fillStyle = needle;
      ctx.fill();
      ctx.restore();
    }
  }

  function setActiveMetric(phase) {
    el.mPingBox.classList.toggle("active", phase === 0);
    el.mDownBox.classList.toggle("active", phase === 2);
    el.mUpBox.classList.toggle("active", phase === 3);
  }

  function setBigValue(value, unit, icon) {
    el.gvNum.textContent = value;
    el.gvUnit.textContent = unit;
    el.gvIcon.innerHTML = icon || "";
    el.gvUnit.style.visibility = unit ? "visible" : "hidden";
  }

  function resetForNewRun() {
    S.target = 0;
    S.disp = 0;
    S.ping = null;
    el.mPing.textContent = PLACEHOLDER;
    el.mDown.textContent = PLACEHOLDER;
    el.mUp.textContent = PLACEHOLDER;
    setActiveMetric(-1);
  }

  function enterIdle() {
    S.running = false;
    S.phase = -1;
    el.dial.classList.remove("mode-gauge");
    el.dial.classList.add("mode-start");

    el.mPing.textContent = "";
    el.mDown.textContent = "";
    el.mUp.textContent = "";
    setActiveMetric(-1);
    setSelectorsDisabled(false);
  }

  function enterRunning() {
    S.running = true;
    el.dial.classList.remove("mode-start");
    el.dial.classList.add("mode-gauge");
    setSelectorsDisabled(true);
  }

  function enterDone() {
    S.running = false;
    S.phase = 4;
    el.dial.classList.remove("mode-gauge");
    el.dial.classList.add("mode-start");
    setActiveMetric(-1);
    setSelectorsDisabled(false);
  }

  function setSelectorsDisabled(d) {
    el.selProv.disabled = d;
    el.selOper.disabled = d;
    el.selNode.disabled = d;
  }

  function updateServerCard() {
    const n = S.filtered[Number(el.selNode.value)];
    if (n) {
      el.srvName.textContent = n.hostname;
      el.srvSub.textContent = `${n.pname} · ${n.hostip}:${n.port}`;
      el.ispName.textContent = n.oper ? `中国${n.oper}` : "未知运营商";
      return;
    }

    if (S.nodes.length) {
      el.srvName.textContent = "无匹配节点";
      el.srvSub.textContent = "请调整省份 / 运营商筛选";
    } else if (S.nodesError) {
      el.srvName.textContent = "节点获取失败";
      el.srvSub.textContent = "点击重试";
    } else {
      el.srvName.textContent = "正在获取节点列表…";
      el.srvSub.textContent = "需要联网";
    }
    el.ispName.textContent = "—";
  }

  let lastT = 0;
  function loop(t) {
    const dt = lastT ? Math.min(0.1, (t - lastT) / 1000) : 0.016;
    lastT = t;

    const k = 1 - Math.exp(-9.0 * dt);
    S.disp += (S.target - S.disp) * k;
    if (Math.abs(S.disp - S.target) < 0.02) S.disp = S.target;

    if (S.running) {
      if (S.phase === 0) {
        setBigValue(S.ping === null ? "0" : S.ping.toFixed(0), "毫秒", ICON.ping);
      } else {
        setBigValue(S.disp.toFixed(2), "兆/秒", S.phase === 3 ? ICON.up : ICON.down);
      }
    }

    drawGauge();
    requestAnimationFrame(loop);
  }

  function bindBackendEvents() {
    bridge.on("phase", (p) => {
      if (!S.running) {
        resetForNewRun();
        enterRunning();
      }
      S.phase = p.idx;
      S.target = 0;
      S.disp = 0;
      setActiveMetric(p.idx);
    });

    bridge.on("ping-step", (p) => {
      if (p.rtt >= 0) {
        S.ping = p.rtt;
        el.mPing.textContent = p.rtt.toFixed(0);
      }
    });

    bridge.on("ping", (p) => {
      S.ping = p.ping;
      el.mPing.textContent = p.ping > 0 ? p.ping.toFixed(0) : PLACEHOLDER;
    });

    bridge.on("instant", (p) => {
      S.target = p.mbps;
    });

    bridge.on("sample", (p) => {
      if (p.preheat) return;
      if (p.phase === 2) {
        el.mDown.textContent = p.mbps.toFixed(2);
      } else {
        el.mUp.textContent = p.mbps.toFixed(2);
      }
    });

    bridge.on("result", (p) => {
      if (typeof p.down === "number") {
        el.mDown.textContent = p.down.toFixed(2);
        S.target = p.down;
      }
      if (typeof p.up === "number") {
        el.mUp.textContent = p.up.toFixed(2);
      }
    });

    bridge.on("done", (p) => {
      if (p && p.error) toast(p.error, true);
      enterDone();
    });

    bridge.on("nodes-updated", (n) => {
      S.nodesError = "";
      loadNodes();
      if (typeof n === "number" && n > 0) toast(`已获取 ${n} 个测速节点`);
    });

    bridge.on("nodes-error", (msg) => {
      S.nodesError = String(msg || "获取失败");
      updateServerCard();
      toast("节点列表获取失败：" + S.nodesError, true);
    });
  }

  function rebuildNodeSelect() {
    const prov = el.selProv.value;
    const oper = el.selOper.value;
    S.filtered = S.nodes.filter(
      (n) => (!prov || n.pname === prov) && (!oper || n.oper === oper)
    );
    el.selNode.innerHTML = "";
    if (!S.filtered.length) {
      el.selNode.appendChild(opt("", "无匹配节点"));
      updateServerCard();
      return;
    }
    S.filtered.forEach((n, i) => {
      el.selNode.appendChild(opt(String(i), `${n.hostname} · ${n.hostip}:${n.port}`));
    });
    updateServerCard();
  }

  function opt(value, label) {
    const o = document.createElement("option");
    o.value = value;
    o.textContent = label;
    return o;
  }

  function fillPickers() {
    const provs = [...new Set(S.nodes.map((n) => n.pname).filter(Boolean))];
    const opers = [...new Set(S.nodes.map((n) => n.oper).filter(Boolean))];
    const keepProv = el.selProv.value;
    const keepOper = el.selOper.value;

    el.selProv.innerHTML = "";
    el.selProv.appendChild(opt("", "全部省份"));
    provs.forEach((p) => el.selProv.appendChild(opt(p, p)));

    el.selOper.innerHTML = "";
    el.selOper.appendChild(opt("", "全部运营商"));
    opers.forEach((o) => el.selOper.appendChild(opt(o, o)));

    if (keepProv && provs.includes(keepProv)) el.selProv.value = keepProv;
    if (keepOper && opers.includes(keepOper)) el.selOper.value = keepOper;

    rebuildNodeSelect();
  }

  async function loadNodes() {
    let list = [];
    try {
      list = await bridge.getNodes();
    } catch (e) {
      S.nodesError = String(e);
    }
    S.nodes = Array.isArray(list) ? list : [];
    fillPickers();
    if (S.nodes.length) {
      await applySavedConfig();
    }
  }

  async function retryNodes(showToast) {
    el.srvSub.textContent = "正在获取…";
    try {
      const n = await bridge.refreshNodes();
      S.nodesError = "";
      await loadNodes();
      if (showToast) toast(`节点列表已更新 · ${n} 个`);
      return n;
    } catch (e) {
      S.nodesError = String(e);
      updateServerCard();
      toast("节点列表获取失败：" + e, true);
      return 0;
    }
  }

  function setSelect(sel, value) {
    if (!value) return false;
    for (const o of sel.options) {
      if (o.value === value) {
        sel.value = value;
        return true;
      }
    }
    return false;
  }

  async function applySavedConfig() {
    let cfg = null;
    try {
      cfg = await bridge.loadConfig();
    } catch (e) {
      return;
    }
    if (!cfg || !cfg.hostip) return;

    setSelect(el.selProv, cfg.province);
    setSelect(el.selOper, cfg.oper);
    rebuildNodeSelect();

    const find = () =>
      S.filtered.findIndex(
        (n) => n.hostip === cfg.hostip && (!cfg.port || n.port === cfg.port)
      );

    let idx = find();
    if (idx < 0) {

      el.selProv.value = "";
      el.selOper.value = "";
      rebuildNodeSelect();
      idx = find();
    }
    if (idx >= 0) {
      el.selNode.value = String(idx);
      updateServerCard();
    }
  }

  async function persistSelection() {
    const n = S.filtered[Number(el.selNode.value)];
    if (!n) return;
    try {
      await bridge.saveConfig({
        province: el.selProv.value || "",
        oper: el.selOper.value || "",
        hostip: n.hostip,
        port: n.port,
      });
    } catch (e) {

    }
  }

  let openedSheet = null;

  function openSheet(node) {
    openedSheet = node;
    el.sheetMask.classList.add("show");
    node.classList.add("show");
  }

  function closeSheet() {
    if (openedSheet) openedSheet.classList.remove("show");
    openedSheet = null;
    el.sheetMask.classList.remove("show");
  }

  async function startTest(mode) {
    if (S.running) return;
    closeSheet();
    const node = S.filtered[Number(el.selNode.value)];
    if (!node) {
      toast("请先选择一个测速节点", true);
      openSheet(el.nodeSheet);
      return;
    }
    persistSelection();
    resetForNewRun();
    enterRunning();
    try {
      await bridge.startTest(node, mode);
    } catch (e) {
      toast(String(e), true);
      enterIdle();
    }
  }

  function bindUI() {
    el.dial.addEventListener("click", () => startTest("full"));

    el.serverRow.addEventListener("click", () => {
      if (S.running) return;

      if (!S.nodes.length) {
        retryNodes(false);
        return;
      }
      openSheet(el.nodeSheet);
    });

    el.moreBtn.addEventListener("click", () => openSheet(el.menuSheet));

    el.sheetMask.addEventListener("click", closeSheet);
    el.sheetCancel.addEventListener("click", closeSheet);
    el.sheetConfirm.addEventListener("click", () => {
      updateServerCard();
      persistSelection();
      closeSheet();
    });
    el.menuClose.addEventListener("click", closeSheet);

    el.menuPingOnly.addEventListener("click", () => startTest("ping"));
    el.menuDownOnly.addEventListener("click", () => startTest("down"));
    el.menuUpOnly.addEventListener("click", () => startTest("up"));

    el.selProv.addEventListener("change", () => {
      rebuildNodeSelect();
      persistSelection();
    });
    el.selOper.addEventListener("change", () => {
      rebuildNodeSelect();
      persistSelection();
    });
    el.selNode.addEventListener("change", () => {
      updateServerCard();
      persistSelection();
    });

    el.menuRefresh.addEventListener("click", async () => {
      closeSheet();
      el.moreBtn.classList.add("spinning");
      try {
        await retryNodes(true);
      } finally {
        el.moreBtn.classList.remove("spinning");
      }
    });

    window.addEventListener("resize", () => fitCanvas(el.gauge));
  }

  async function init() {
    bindBackendEvents();
    bindUI();
    enterIdle();
    requestAnimationFrame(loop);
    await loadNodes();
  }

  init();
})();
