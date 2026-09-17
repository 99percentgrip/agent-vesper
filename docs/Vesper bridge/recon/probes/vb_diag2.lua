-- Vesper Bridge diag2: enumerate MP4 + MKV + QuickTime codecs by NAME, and probe
-- which SetCurrentRenderFormatAndCodec combos the free build actually accepts.
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 240 then s = s:sub(1, 240) .. "~" end
  return s
end

local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
if not p then print("VBERR|diag2|no project") return end

local fmts = p:GetRenderFormats()
for ext, _ in pairs({ mp4 = 1, mkv = 1, mov = 1 }) do
  -- find format KEY by extension
  local key = nil
  for k, v in pairs(fmts or {}) do
    if tostring(v):lower() == ext then key = k break end
  end
  if not key then
    print("VBOK|diag2|" .. ext .. "|no_format_key")
  else
    local codecs = p:GetRenderCodecs(ext) or {}
    local names = {}
    for cid, cname in pairs(codecs) do
      names[#names + 1] = esc(cid) .. ":" .. esc(cname)
    end
    table.sort(names)
    print("VBOK|diag2|" .. ext .. "|" .. table.concat(names, ","))
  end
end
