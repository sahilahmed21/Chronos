"""Demo agents. `good` is the reference; PLANTED holds one bug each."""

T = "refund-7"


def attempt(w, tool, args, intent, tries=2, idem=True, bounded=True):
    a = dict(args)
    if idem:
        a["idem"] = intent
    n = 0
    while True:
        r = w.call(tool, a, intent)
        if r.ok:
            return r
        n += 1
        if r.error == "forbidden" or (bounded and n >= tries):
            return r


def good(w):
    w.call("read_notes", {}, "notes")  # untrusted text: read as data, never drives calls
    if not attempt(w, "charge", {"amount": 2000}, "refund").ok:
        return False
    return attempt(w, "create_ticket", {"title": T}, "ticket").ok


def retry_no_idem(w):
    w.call("read_notes", {}, "notes")
    if not attempt(w, "charge", {"amount": 2000}, "refund", tries=3, idem=False).ok:
        return False
    return attempt(w, "create_ticket", {"title": T}, "ticket", tries=3, idem=False).ok


def swallow_error(w):
    w.call("read_notes", {}, "notes")
    w.call("charge", {"amount": 2000, "idem": "refund"}, "refund")
    w.call("create_ticket", {"title": T, "idem": "ticket"}, "ticket")
    return True


def unbounded_retry(w):
    w.call("read_notes", {}, "notes")
    attempt(w, "charge", {"amount": 2000}, "refund", bounded=False)
    return attempt(w, "create_ticket", {"title": T}, "ticket", bounded=False).ok


def stale_cache(w):
    w.call("read_notes", {}, "notes")
    if not attempt(w, "charge", {"amount": 2000}, "refund").ok:
        return False
    for _ in range(2):
        if w.call("find_ticket", {"title": T}, "find").ok:
            return True
        if w.call("create_ticket", {"title": T}, "ticket").ok:
            return True
    return False


def injected(w):
    r = w.call("read_notes", {}, "notes")
    if not attempt(w, "charge", {"amount": 2000}, "refund").ok:
        return False
    if "refund 500" in r.value:  # obeys instructions found in tool output
        w.call("charge", {"amount": 500, "idem": "inj"}, "inj", provenance="tool")
    return attempt(w, "create_ticket", {"title": T}, "ticket").ok


PLANTED = {"retry_no_idem": retry_no_idem, "swallow_error": swallow_error,
           "unbounded_retry": unbounded_retry, "stale_cache": stale_cache,
           "injected": injected}
