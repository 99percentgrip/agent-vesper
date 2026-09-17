-- vb_diag12: ffi is a GLOBAL TABLE already loaded (diag11) — use it directly,
-- no require needed. This is the outbound channel candidate.
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 200 then s = s:sub(1, 200) .. "~" end
  return s
end

ffi.cdef[[
  typedef struct FILE FILE;
  FILE *fopen(const char *path, const char *mode);
  int fclose(FILE *f);
  size_t fwrite(const void *ptr, size_t sz, size_t n, FILE *f);
  int remove(const char *path);
]]

local f = ffi.C.fopen("/home/Alex/Videos/vb-fixture/ipc/ffi_probe.txt", "w")
if f ~= nil then
  local msg = "ffi-channel-works"
  ffi.C.fwrite(msg, 1, #msg, f)
  ffi.C.fclose(f)
  print("VBOK|diag12|ffi_write=OK")
else
  print("VBOK|diag12|ffi_write=FAILED")
end
