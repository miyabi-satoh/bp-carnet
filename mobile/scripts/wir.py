#!/usr/bin/env python3
"""シミュレータで動いているアプリの WKWebView で JS を評価し、結果を出す。

    mobile/scripts/wir.py '<JS>'

- シミュレータの Web Inspector のソケット (webinspectord_sim) に直接つなぐ。追加のインストールは要らない。
- 評価の前に同じフォルダの wir-helpers.js を読み込む (`__h.click` など)。
- 結果が Promise なら、決着を待って値を出す。
- アプリは Debug ビルドであること (Capacitor はリリースビルドの WebView を検査させない)。
- 起動したばかりのシミュレータでは、最初の1回が時間切れになることがある。そのときはもう一度実行する。
- 環境変数 WIR_APP でアプリの bundle id を変えられる。シミュレータを2台以上起動しているときは、どれにつながるか決まらない。
"""
import json
import os
import plistlib
import socket
import struct
import subprocess
import sys
import time
import uuid

BUNDLE = os.environ.get("WIR_APP", "com.amiiby.bpcarnet")
HELPERS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "wir-helpers.js")


def find_socket():
    out = subprocess.run(["lsof", "-U"], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if "webinspectord_sim.socket" in line:
            return line.split()[-1]
    sys.exit("webinspectord_sim.socket が見つからない (シミュレータは起動している?)")


def main():
    # 文を並べた JS も受け取れるよう eval に通し、最後の文の値を Promise にそろえる。
    with open(HELPERS) as f:
        expr = f.read() + "\n;Promise.resolve(eval(" + json.dumps(sys.argv[1]) + "))"

    s = socket.socket(socket.AF_UNIX)
    s.connect(find_socket())
    s.settimeout(60)
    connection, sender = str(uuid.uuid4()).upper(), str(uuid.uuid4()).upper()

    # 1通は「4バイトのビッグエンディアンの長さ + バイナリ plist」。
    def send(selector, argument):
        argument["WIRConnectionIdentifierKey"] = connection
        data = plistlib.dumps({"__selector": selector, "__argument": argument}, fmt=plistlib.FMT_BINARY)
        s.sendall(struct.pack(">I", len(data)) + data)

    def recv_exact(n):
        buf = b""
        while len(buf) < n:
            chunk = s.recv(n - len(buf))
            if not chunk:
                sys.exit("Web Inspector の接続が切れた")
            buf += chunk
        return buf

    def recv():
        return plistlib.loads(recv_exact(struct.unpack(">I", recv_exact(4))[0]))

    app = page = target = None

    def to_target(message):
        send("_rpc_forwardSocketData:", {
            "WIRApplicationIdentifierKey": app, "WIRPageIdentifierKey": page, "WIRSenderKey": sender,
            "WIRSocketDataKey": json.dumps({"id": 1, "method": "Target.sendMessageToTarget", "params": {
                "targetId": target, "message": json.dumps(message)}}).encode()})

    def list_pages(app_id):
        send("_rpc_forwardGetListing:", {"WIRApplicationIdentifierKey": app_id})

    send("_rpc_reportIdentifier:", {})
    try:
        while True:
            m = recv()
            a, selector = m.get("__argument", {}), m["__selector"]
            if app is None and a.get("WIRApplicationDictionaryKey"):
                for key, info in a["WIRApplicationDictionaryKey"].items():
                    if info.get("WIRApplicationBundleIdentifierKey") == BUNDLE:
                        app = key
                if app:
                    list_pages(app)
            elif (app is None and selector == "_rpc_applicationConnected:"
                  and a.get("WIRApplicationBundleIdentifierKey") == BUNDLE):
                app = a["WIRApplicationIdentifierKey"]
                list_pages(app)
            elif page is None and "WIRListingKey" in a and a.get("WIRApplicationIdentifierKey") == app:
                # 起動の途中は about:blank だけのことがあるので、アプリの画面が載るまで取り直す。
                pages = [p for p in a["WIRListingKey"].values()
                         if str(p.get("WIRURLKey", "")).startswith("capacitor://")]
                if not pages:
                    time.sleep(0.2)
                    list_pages(app)
                    continue
                page = pages[0]["WIRPageIdentifierKey"]
                send("_rpc_forwardSocketSetup:", {
                    "WIRApplicationIdentifierKey": app, "WIRPageIdentifierKey": page,
                    "WIRSenderKey": sender, "WIRAutomaticallyPause": False})
            elif selector == "_rpc_applicationSentData:":
                # 同じシミュレータを Safari などが検査していても、自分あての応答だけを読む。
                if a.get("WIRDestinationKey", sender) != sender:
                    continue
                d = json.loads(a["WIRMessageDataKey"])
                if d.get("method") == "Target.targetCreated" and target is None:
                    target = d["params"]["targetInfo"]["targetId"]
                    to_target({"id": 2, "method": "Runtime.evaluate",
                               "params": {"expression": expr, "returnByValue": False}})
                elif d.get("method") == "Target.dispatchMessageFromTarget":
                    inner = json.loads(d["params"]["message"])
                    result = inner.get("result", {})
                    if (inner.get("id") == 2 and not result.get("wasThrown")
                            and "objectId" in result.get("result", {})):
                        # WebKit の Runtime.evaluate は Promise を待たないので、awaitPromise で待つ。
                        to_target({"id": 3, "method": "Runtime.awaitPromise", "params": {
                            "promiseObjectId": result["result"]["objectId"], "returnByValue": True}})
                    elif inner.get("id") in (2, 3):
                        if "error" in inner or result.get("wasThrown"):
                            sys.exit("JS のエラー: " + json.dumps(inner.get("error") or result.get("result"),
                                                                ensure_ascii=False))
                        value = result.get("result", {}).get("value")
                        print(value if isinstance(value, str) else json.dumps(value, ensure_ascii=False))
                        return
    except socket.timeout:
        sys.exit(f"時間切れ (app={app} page={page} target={target})。もう一度実行する")


main()
