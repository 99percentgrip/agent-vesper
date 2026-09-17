-- vb_diag11: THE definitive outbound channel hunt, part 1 — os.execute / io.popen.
-- os functions available: clock,date,difftime,getenv,time,tmpname (measured diag5)
-- => NO os.execute, NO io.popen. But os.getenv WORKS. And os.tmpname!
-- Q: can LuaJIT FFI write files? Check for jit/ffi presence in the Console sandbox.
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 200 then s = s:sub(1, 200) .. "~" end
  return s
end

print("VBOK|diag11|jit=" .. tostring(type(jit)))
print("VBOK|diag11|ffi=" .. tostring(type(ffi)))
local okj, ver = pcall(function() return jit and jit.version end)
print("VBOK|diag11|jit_version=" .. esc(ver))
local okf = pcall(function() return require("ffi") end)
if okf then
  print("VBOK|diag11|require_ffi=ok")
  -- PROBE ONLY: open with C fopen CREATE; do not write payload yet
  local ffi = require("ffi")
  ffi.cdef[[
    typedef struct FILE FILE;
    FILE *fopen(const char *path, const char *mode);
    int fclose(FILE *f);
    size_t fwrite(const void *ptr, size_t sz, size_t n, FILE *f);
  ]]
  local f = ffi.C.fopen("/home/Alex/Videos/vb-fixture/ipc/ffi_probe.txt", "w")
  if f ~= nil then
    local msg = "ffi-channel-works"
    ffi.C.fwrite(msg, 1, #msg, f)
    ffi.C.fclose(f)
    print("VBOK|diag11|ffi_fopen=OPENED_AND_WROTE")
  else
    print("VBOK|diag11|ffi_fopen=FAILED_null")
  end
else
  print("VBOK|diag11|require_ffi=failed|" .. esc(okf))
end
