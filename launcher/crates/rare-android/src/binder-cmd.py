# Runs an Android service's shell command (like `cmd package ...`) over binder, as this user,
# which Android sees as its system user. Prints the command's output; exits with its status.
import configparser, ctypes, os, sys

def main():
    c = configparser.ConfigParser()
    c.read("/var/lib/waydroid/waydroid.cfg")
    w = c["waydroid"] if "waydroid" in c else {}
    dev = "/dev/" + w.get("binder", "binder")
    smp = w.get("service_manager_protocol", "aidl3")
    bp = w.get("binder_protocol", "aidl3")
    g = ctypes.CDLL("libgbinder.so.1")
    vp = ctypes.c_void_p
    g.gbinder_servicemanager_new2.restype = vp
    g.gbinder_servicemanager_new2.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_char_p]
    g.gbinder_servicemanager_get_service_sync.restype = vp
    g.gbinder_servicemanager_get_service_sync.argtypes = [vp, ctypes.c_char_p, ctypes.POINTER(ctypes.c_int)]
    g.gbinder_client_new.restype = vp
    g.gbinder_client_new.argtypes = [vp, ctypes.c_char_p]
    g.gbinder_client_new_request.restype = vp
    g.gbinder_client_new_request.argtypes = [vp]
    g.gbinder_local_request_init_writer.argtypes = [vp, vp]
    g.gbinder_writer_append_fd.argtypes = [vp, ctypes.c_int]
    g.gbinder_writer_append_int32.argtypes = [vp, ctypes.c_uint32]
    g.gbinder_writer_append_string16.argtypes = [vp, ctypes.c_char_p]
    g.gbinder_writer_append_remote_object.argtypes = [vp, vp]
    g.gbinder_local_request_unref.argtypes = [vp]
    g.gbinder_remote_reply_unref.argtypes = [vp]
    g.gbinder_client_transact_sync_reply.restype = vp
    g.gbinder_client_transact_sync_reply.argtypes = [vp, ctypes.c_uint32, vp, ctypes.POINTER(ctypes.c_int)]
    sm = g.gbinder_servicemanager_new2(dev.encode(), smp.encode(), bp.encode())
    if not sm:
        print("no binder", file=sys.stderr)
        return 3
    st = ctypes.c_int(0)
    remote = g.gbinder_servicemanager_get_service_sync(sm, sys.argv[1].encode(), ctypes.byref(st))
    if not remote:
        print("no service " + sys.argv[1], file=sys.stderr)
        return 2
    client = g.gbinder_client_new(remote, None)
    req = g.gbinder_client_new_request(client)
    writer = ctypes.create_string_buffer(256)
    g.gbinder_local_request_init_writer(req, writer)
    rin, win = os.pipe()
    os.close(win)
    rout, wout = os.pipe()
    g.gbinder_writer_append_fd(writer, rin)
    g.gbinder_writer_append_fd(writer, wout)
    g.gbinder_writer_append_fd(writer, wout)
    args = sys.argv[2:]
    g.gbinder_writer_append_int32(writer, len(args))
    for a in args:
        g.gbinder_writer_append_string16(writer, a.encode())
    g.gbinder_writer_append_remote_object(writer, None)
    g.gbinder_writer_append_remote_object(writer, None)
    code = (ord("_") << 24) | (ord("C") << 16) | (ord("M") << 8) | ord("D")
    reply = g.gbinder_client_transact_sync_reply(client, code, req, ctypes.byref(st))
    # The request holds copies of the pipe ends: drop it, or the output never ends.
    g.gbinder_local_request_unref(req)
    if reply:
        g.gbinder_remote_reply_unref(reply)
    os.close(wout)
    os.close(rin)
    out = b""
    while True:
        b = os.read(rout, 65536)
        if not b:
            break
        out += b
    sys.stdout.write(out.decode(errors="replace"))
    return 0 if st.value == 0 else 1

sys.exit(main())
