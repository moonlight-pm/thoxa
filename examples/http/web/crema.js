/* Little Crema swap runtime (example-only). Progressive: no attrs → full page. */
(function () {
    function attr(element, name) {
        return element.getAttribute(name);
    }

    function swap(node, html, how) {
        if (how === "outer") {
            node.outerHTML = html;
        } else if (how === "append") {
            node.insertAdjacentHTML("beforeend", html);
        } else {
            node.innerHTML = html;
        }
    }

    function queryUrl(base, form) {
        var params = new URLSearchParams(new FormData(form));
        var query = params.toString();
        if (query.length === 0) {
            return base;
        }
        var join = base.indexOf("?") >= 0 ? "&" : "?";
        return base + join + query;
    }

    function syncSearchBox(pageUrl) {
        var box = document.querySelector('input[name="q"]');
        if (!box) {
            return;
        }
        var parsed = new URL(pageUrl, window.location.origin);
        box.value = parsed.searchParams.get("q") || "";
    }

    function apply(html, targetSel, how, pageUrl, fragmentUrl) {
        var target = document.querySelector(targetSel);
        if (!target) {
            return;
        }
        swap(target, html, how);
        if (pageUrl) {
            history.pushState(
                { thxUrl: fragmentUrl, thxTarget: targetSel, thxSwap: how },
                "",
                pageUrl
            );
            syncSearchBox(pageUrl);
        }
    }

    function load(fragmentUrl, targetSel, how, pageUrl) {
        return fetch(fragmentUrl, { headers: { Accept: "text/html" } }).then(function (response) {
            if (!response.ok) {
                window.location.assign(pageUrl || fragmentUrl);
                return;
            }
            return response.text().then(function (html) {
                apply(html, targetSel, how, pageUrl, fragmentUrl);
            });
        });
    }

    function onGet(element) {
        var targetSel = attr(element, "thx-target");
        if (!targetSel) {
            return;
        }
        var how = attr(element, "thx-swap") || "inner";
        var fragmentUrl = attr(element, "thx-get");
        var pageUrl = null;
        if (element.tagName === "FORM") {
            fragmentUrl = queryUrl(fragmentUrl || element.getAttribute("action"), element);
            if (element.hasAttribute("thx-push")) {
                pageUrl = queryUrl(element.getAttribute("action") || window.location.pathname, element);
            }
        } else {
            fragmentUrl = fragmentUrl || element.getAttribute("href");
            if (element.hasAttribute("thx-push")) {
                pageUrl = element.getAttribute("href");
            }
        }
        load(fragmentUrl, targetSel, how, pageUrl);
    }

    function onPost(form) {
        var targetSel = attr(form, "thx-target");
        if (!targetSel) {
            return;
        }
        var how = attr(form, "thx-swap") || "inner";
        var postUrl = attr(form, "thx-post") || form.getAttribute("action");
        fetch(postUrl, {
            method: "POST",
            headers: { "Content-Type": "application/x-www-form-urlencoded" },
            body: new URLSearchParams(new FormData(form)),
            redirect: "follow"
        }).then(function (response) {
            if (!response.ok) {
                var target = document.querySelector(targetSel);
                if (target) {
                    target.innerHTML = '<p class="muted">Could not place that order.</p>';
                }
                return;
            }
            if (!response.redirected) {
                return response.text().then(function (html) {
                    apply(html, targetSel, how, null, postUrl);
                });
            }
            var ticketUrl = response.url.replace(/\/?$/, "/ticket");
            return load(ticketUrl, targetSel, how, null);
        });
    }

    document.addEventListener("submit", function (event) {
        var form = event.target;
        if (!(form instanceof HTMLFormElement)) {
            return;
        }
        if (form.hasAttribute("thx-get")) {
            event.preventDefault();
            onGet(form);
        } else if (form.hasAttribute("thx-post")) {
            event.preventDefault();
            onPost(form);
        }
    });

    document.addEventListener("click", function (event) {
        if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
            return;
        }
        var link = event.target.closest("a[thx-get]");
        if (!link) {
            return;
        }
        event.preventDefault();
        onGet(link);
    });

    window.addEventListener("popstate", function (event) {
        var state = event.state;
        if (!state || !state.thxUrl || !state.thxTarget) {
            return;
        }
        load(state.thxUrl, state.thxTarget, state.thxSwap || "inner", null);
        syncSearchBox(window.location.href);
    });

    document.querySelectorAll("[thx-sse]").forEach(function (element) {
        var url = attr(element, "thx-sse");
        var targetSel = attr(element, "thx-target");
        if (!url || !targetSel) {
            return;
        }
        var how = attr(element, "thx-swap") || "inner";
        var source = new EventSource(url);
        source.onmessage = function (event) {
            apply(event.data, targetSel, how, null, url);
        };
    });
})();
