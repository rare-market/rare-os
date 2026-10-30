#!/usr/bin/env python3
"""A small client for Rare's network Local API (docs/LOCAL-API.md, "Network apps").

No dependencies, Python 3.8+. For a script or an AI agent on a laptop:

    from rare_netapi import Rare
    rare = Rare("suiplay.local", token)         # the token is shown in Settings > Developer
    rare.capabilities()                         # which switches the owner turned on
    rare.library()["running"]
    rare.launch("steam:620")
    rare.tap("a"); rare.tap("down"); rare.stick("left_x", 1.0)
    open("screen.webp", "wb").write(rare.screenshot())
    rare.sell("0x...")                          # waits for the owner on the device

Or from a shell (RARE_HOST and RARE_TOKEN in the environment):

    rare_netapi.py caps | me | inventory | library
    rare_netapi.py launch <id> | close [id] | open <view> [id]
    rare_netapi.py tap <button> | stick <axis> <value> | shot <file>
    rare_netapi.py sell <itemId> | buy <itemId>

Every capability is a separate switch on the device and off by default. A call whose switch is off raises
RareError with code "capability_off" and the switch's id in .capability: ask the owner to turn it on.
Buying and selling only ever opens a review sheet on the device; the call returns when the owner is done.
"""
import json
import os
import sys
import urllib.error
import urllib.request

PORT = 47100


class RareError(Exception):
    def __init__(self, status, code, message, capability=None):
        super().__init__(f"{status} {code}: {message}")
        self.status, self.code, self.message, self.capability = status, code, message, capability


class Rare:
    def __init__(self, host, token, port=PORT, timeout=30):
        self.base = f"http://{host}:{port}"
        self.token = token
        self.timeout = timeout

    def _call(self, method, path, body=None, timeout=None, raw=False):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.base + path, data=data, method=method)
        req.add_header("Authorization", "Bearer " + self.token)
        if data is not None:
            req.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(req, timeout=timeout or self.timeout) as r:
                payload = r.read()
                return payload if raw else json.loads(payload or b"{}")
        except urllib.error.HTTPError as e:
            try:
                err = json.loads(e.read()).get("error", {})
            except ValueError:
                err = {}
            raise RareError(e.code, err.get("code", "http"), err.get("message", e.reason), err.get("capability")) from None

    # What is on.
    def capabilities(self):
        """The switches and whether each is on: [{id, title, about, enabled, routes}]."""
        return self._call("GET", "/v1/capabilities")["capabilities"]

    # Read profile
    def me(self):
        return self._call("GET", "/v1/me")

    # Read inventory and library
    def inventory(self):
        return self._call("GET", "/v1/inventory")["items"]

    def library(self):
        return self._call("GET", "/v1/library")

    # Launch and close games and apps
    def launch(self, id):
        return self._call("POST", "/v1/launch", {"id": id})

    def close(self, id=None):
        return self._call("POST", "/v1/close", {} if id is None else {"id": id})

    def open(self, view, id=None):
        return self._call("POST", "/v1/open", {"view": view, **({} if id is None else {"id": id})})

    # Send controller input
    def input(self, *events):
        return self._call("POST", "/v1/input", {"events": list(events)})

    def tap(self, button, ms=80):
        return self.input({"button": button, "action": "tap", "ms": ms})

    def down(self, button):
        return self.input({"button": button, "action": "down"})

    def up(self, button):
        return self.input({"button": button, "action": "up"})

    def stick(self, axis, value):
        return self.input({"axis": axis, "value": value})

    # Request purchases and listings (each waits for the owner, up to 10 minutes)
    def sell(self, item_id, mode="best-offer", price_mist=None):
        body = {"itemId": item_id, "mode": mode}
        if price_mist is not None:
            body["priceMist"] = str(price_mist)
        return self._call("POST", "/v1/sell", body, timeout=620)

    def buy(self, item_id=None, collection=None, max_mist=None):
        body = {}
        if item_id:
            body["itemId"] = item_id
        if collection:
            body["collection"] = collection
        if max_mist is not None:
            body["maxMist"] = str(max_mist)
        return self._call("POST", "/v1/buy", body, timeout=620)

    # Screenshots
    def screenshot(self):
        """The screen as image/webp bytes."""
        return self._call("GET", "/v1/screenshot", raw=True)


def main(argv):
    host, token = os.environ.get("RARE_HOST", "suiplay.local"), os.environ.get("RARE_TOKEN", "")
    if not argv or not token:
        print(__doc__)
        return 2
    rare, cmd, args = Rare(host, token), argv[0], argv[1:]
    try:
        if cmd == "caps":
            for c in rare.capabilities():
                print(("on  " if c["enabled"] else "off ") + c["id"].ljust(11) + c["title"])
            return 0
        if cmd == "shot":
            data = rare.screenshot()
            open(args[0] if args else "screen.webp", "wb").write(data)
            print(f"{len(data)} bytes")
            return 0
        simple = {
            "me": lambda: rare.me(),
            "inventory": lambda: rare.inventory(),
            "library": lambda: rare.library(),
            "launch": lambda: rare.launch(args[0]),
            "close": lambda: rare.close(*args[:1]),
            "open": lambda: rare.open(*args[:2]),
            "tap": lambda: rare.tap(args[0]),
            "stick": lambda: rare.stick(args[0], float(args[1])),
            "sell": lambda: rare.sell(args[0]),
            "buy": lambda: rare.buy(args[0]),
        }
        if cmd not in simple:
            print(__doc__)
            return 2
        print(json.dumps(simple[cmd](), indent=2))
        return 0
    except RareError as e:
        if e.code == "capability_off":
            print(f"Off on the device: {e.capability}. Ask the owner to turn it on in Settings > Developer.", file=sys.stderr)
        else:
            print(e, file=sys.stderr)
        return 1
    except IndexError:
        print(__doc__)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
