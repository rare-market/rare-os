#!/usr/bin/env python3
"""A software FIDO2 security key over /dev/uhid (Linux), for testing Rare Wallet's USB passkey path.

CTAPHID framing, CTAP2 getInfo / makeCredential (ES256, resident) / getAssertion / selection /
clientPIN (PIN/UV auth protocol 1, the key's PIN is 1234), attestation "none", user presence always
given. Keys live in memory only.
Run as root (uhid); prints the hidraw node it made and makes it readable by everyone.
Throwaway test tool: never use for real accounts.
"""
import glob, hashlib, os, struct, sys, time

import hmac
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

# ---------------------------------------------------------------- CBOR (the subset CTAP uses)

def cbor_enc(v):
    def head(major, n):
        if n < 24:
            return bytes([major << 5 | n])
        for ai, fmt in ((24, ">B"), (25, ">H"), (26, ">I"), (27, ">Q")):
            if n < 1 << (8 * struct.calcsize(fmt)):
                return bytes([major << 5 | ai]) + struct.pack(fmt, n)
    if isinstance(v, bool):
        return b"\xf5" if v else b"\xf4"
    if v is None:
        return b"\xf6"
    if isinstance(v, int):
        return head(0, v) if v >= 0 else head(1, -1 - v)
    if isinstance(v, (bytes, bytearray)):
        return head(2, len(v)) + bytes(v)
    if isinstance(v, str):
        b = v.encode()
        return head(3, len(b)) + b
    if isinstance(v, list):
        return head(4, len(v)) + b"".join(cbor_enc(x) for x in v)
    if isinstance(v, dict):
        # canonical order: shorter encoded key first, then bytewise
        items = sorted(((cbor_enc(k), cbor_enc(x)) for k, x in v.items()), key=lambda kv: (len(kv[0]), kv[0]))
        return head(5, len(items)) + b"".join(k + x for k, x in items)
    raise TypeError(type(v))


def cbor_dec(b, i=0):
    ib = b[i]
    major, ai = ib >> 5, ib & 31
    i += 1
    if ai < 24:
        n = ai
    elif ai in (24, 25, 26, 27):
        size = 1 << (ai - 24)
        n = int.from_bytes(b[i:i + size], "big")
        i += size
    else:
        raise ValueError("indefinite")
    if major == 0:
        return n, i
    if major == 1:
        return -1 - n, i
    if major == 2:
        return bytes(b[i:i + n]), i + n
    if major == 3:
        return b[i:i + n].decode(), i + n
    if major == 4:
        out = []
        for _ in range(n):
            x, i = cbor_dec(b, i)
            out.append(x)
        return out, i
    if major == 5:
        out = {}
        for _ in range(n):
            k, i = cbor_dec(b, i)
            x, i = cbor_dec(b, i)
            out[k] = x
        return out, i
    if major == 7:
        return {20: False, 21: True, 22: None}.get(ai), i
    raise ValueError(major)

# ---------------------------------------------------------------- the authenticator

AAGUID = bytes(16)
CREDS = []  # (id, private key, rp id, user dict, counter list)


def sha(b):
    return hashlib.sha256(b).digest()


PIN_HASH = hashlib.sha256(b"1234").digest()[:16]
AGREE = ec.generate_private_key(ec.SECP256R1())
PIN_TOKEN = os.urandom(32)


def cose_pub(key, alg):
    n = key.public_key().public_numbers()
    return {1: 2, 3: alg, -1: 1, -2: n.x.to_bytes(32, "big"), -3: n.y.to_bytes(32, "big")}


def shared_secret(platform):
    pub = ec.EllipticCurvePublicNumbers(int.from_bytes(platform[-2], "big"), int.from_bytes(platform[-3], "big"), ec.SECP256R1()).public_key()
    return hashlib.sha256(AGREE.exchange(ec.ECDH(), pub)).digest()


def aes(key, data, enc):
    c = Cipher(algorithms.AES(key), modes.CBC(bytes(16)))
    x = c.encryptor() if enc else c.decryptor()
    return x.update(data) + x.finalize()


def client_pin(m):
    sub = m.get(2)
    if sub == 0x01:
        return 0, {3: 8}
    if sub == 0x02:
        return 0, {1: cose_pub(AGREE, -25)}
    if sub in (0x05, 0x09):
        k = shared_secret(m[3])
        if aes(k, m[6], False) != PIN_HASH:
            return 0x31, None  # PIN_INVALID
        return 0, {2: aes(k, PIN_TOKEN, True)}
    return 0x01, None


def uv_ok(m, cdh):
    auth = m.get(8)
    if auth is None:
        return None
    return hmac.new(PIN_TOKEN, cdh, hashlib.sha256).digest()[:16] == auth


def get_info():
    return 0, {1: ["FIDO_2_0"], 3: AAGUID, 4: {"rk": True, "up": True, "plat": False, "clientPin": True}, 6: [1], 9: ["usb"]}


def make_credential(m):
    cdh, rp, user = m[1], m[2], m[3]
    uv = uv_ok(m, cdh)
    if uv is False:
        return 0x33, None  # PIN_AUTH_INVALID
    if uv is None:
        return 0x36, None  # PIN_REQUIRED
    if not any(p.get("alg") == -7 for p in m[4]):
        return 0x26, None
    for e in m.get(5, []) or []:
        if any(c[0] == e["id"] for c in CREDS):
            return 0x19, None  # CREDENTIAL_EXCLUDED
    key = ec.generate_private_key(ec.SECP256R1())
    cid = os.urandom(32)
    nums = key.public_key().public_numbers()
    cose = cbor_enc({1: 2, 3: -7, -1: 1, -2: nums.x.to_bytes(32, "big"), -3: nums.y.to_bytes(32, "big")})
    CREDS.append((cid, key, rp["id"], user, [0]))
    auth = sha(rp["id"].encode()) + bytes([0x45]) + (0).to_bytes(4, "big") + AAGUID + len(cid).to_bytes(2, "big") + cid + cose
    print(f"vfido: made a credential for {rp['id']}", flush=True)
    return 0, {1: "none", 2: auth, 3: {}}


def get_assertion(m):
    rp, cdh = m[1], m[2]
    uv = uv_ok(m, cdh)
    if uv is False:
        return 0x33, None
    allow = [c["id"] for c in (m.get(3) or [])]
    cands = [c for c in CREDS if c[2] == rp and (not allow or c[0] in allow)]
    if not cands:
        return 0x2E, None
    cid, key, _, user, ctr = cands[-1]
    ctr[0] += 1
    auth = sha(rp.encode()) + bytes([0x05 if uv else 0x01]) + ctr[0].to_bytes(4, "big")
    sig = key.sign(auth + cdh, ec.ECDSA(hashes.SHA256()))
    out = {1: {"type": "public-key", "id": cid}, 2: auth, 3: sig}
    if not allow:
        out[4] = {"id": user["id"]}
        out[5] = 1
    print(f"vfido: signed for {rp} (counter {ctr[0]})", flush=True)
    return 0, out


def ctap2(req):
    cmd, body = req[0], req[1:]
    m = cbor_dec(body)[0] if body else {}
    if cmd == 0x04:
        st, r = get_info()
    elif cmd == 0x01:
        st, r = make_credential(m)
    elif cmd == 0x02:
        st, r = get_assertion(m)
    elif cmd == 0x06:
        st, r = client_pin(m)
    elif cmd == 0x0B:
        st, r = 0, None
    else:
        print(f"vfido: unsupported CTAP2 command {cmd:#x}", flush=True)
        st, r = 0x01, None
    return bytes([st]) + (cbor_enc(r) if r is not None else b"")

# ---------------------------------------------------------------- uhid and CTAPHID

UHID_DESTROY, UHID_START, UHID_STOP, UHID_OPEN, UHID_CLOSE, UHID_OUTPUT, UHID_CREATE2, UHID_INPUT2 = 1, 2, 3, 4, 5, 6, 11, 12
EVENT = 4 + 4372
RDESC = bytes.fromhex("06D0F10901A10109201500 26FF00750895408102 0921150026FF00750895409102C0".replace(" ", ""))


def create(fd):
    name = b"Rare test security key".ljust(128, b"\0")
    req = name + b"rare-vfido".ljust(64, b"\0") + b"rare-vfido-1".ljust(64, b"\0")
    req += struct.pack("<HHIIII", len(RDESC), 0x03, 0x1209, 0xF1D0, 1, 0) + RDESC.ljust(4096, b"\0")
    os.write(fd, struct.pack("<I", UHID_CREATE2) + req.ljust(4372, b"\0"))


def send(fd, data):
    os.write(fd, (struct.pack("<IH", UHID_INPUT2, len(data)) + data.ljust(4096, b"\0")).ljust(EVENT, b"\0"))


def reply(fd, cid, cmd, payload):
    first = cid + bytes([cmd]) + len(payload).to_bytes(2, "big") + payload[:57]
    send(fd, first.ljust(64, b"\0"))
    rest, seq = payload[57:], 0
    while rest:
        send(fd, (cid + bytes([seq]) + rest[:59]).ljust(64, b"\0"))
        rest, seq = rest[59:], seq + 1


def main():
    fd = os.open("/dev/uhid", os.O_RDWR)
    before = set(glob.glob("/dev/hidraw*"))
    create(fd)
    msg = {}
    next_cid = 0x10000001
    announced = False
    while True:
        ev = os.read(fd, EVENT)
        typ = struct.unpack_from("<I", ev)[0]
        if typ == UHID_START and not announced:
            time.sleep(0.5)
            new = sorted(set(glob.glob("/dev/hidraw*")) - before)
            for n in new:
                os.chmod(n, 0o666)
            print(f"vfido: ready on {' '.join(new)}", flush=True)
            announced = True
            continue
        if typ != UHID_OUTPUT:
            continue
        size = struct.unpack_from("<H", ev, 4 + 4096)[0]
        pkt = ev[4:4 + size]
        if len(pkt) == 65:
            pkt = pkt[1:]
        cid, b = pkt[:4], pkt[4]
        if b & 0x80:
            cmd, n = b, int.from_bytes(pkt[5:7], "big")
            msg[cid] = [cmd, n, bytearray(pkt[7:7 + min(n, 57)])]
        elif cid in msg:
            m = msg[cid]
            m[2] += pkt[5:5 + min(m[1] - len(m[2]), 59)]
        else:
            continue
        cmd, n, data = msg[cid]
        if len(data) < n:
            continue
        del msg[cid]
        data = bytes(data)
        if cmd == 0x86:  # INIT
            new_cid = next_cid.to_bytes(4, "big")
            next_cid += 1
            reply(fd, cid, 0x86, data[:8] + new_cid + bytes([2, 1, 0, 0, 0x05 | 0x08]))
        elif cmd == 0x81:  # PING
            reply(fd, cid, 0x81, data)
        elif cmd == 0x88:  # WINK
            reply(fd, cid, 0x88, b"")
        elif cmd == 0x90:  # CBOR
            reply(fd, cid, 0x90, ctap2(data))
        elif cmd == 0x91:  # CANCEL
            pass
        else:
            reply(fd, cid, 0xBF, bytes([0x01]))


if __name__ == "__main__":
    main()
