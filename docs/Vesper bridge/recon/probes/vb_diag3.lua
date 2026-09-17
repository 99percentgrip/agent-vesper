-- Vesper Bridge diag3: find which (format, codec) pairs SetCurrentRenderFormatAndCodec
-- ACTUALLY accepts on free 21.1 — stop guessing, measure.
local function esc(s)
  s = tostring(s or "?")
  s = s:gsub("[%|\r\n\"%c]", " ")
  if #s > 200 then s = s:sub(1, 200) .. "~" end
  return s
end

local pm = resolve:GetProjectManager()
local p = pm:GetCurrentProject()
if not p then print("VBERR|diag3|no project") return end

local fmts = p:GetRenderFormats()
local results = {}

-- Probe every format/codec pair from the measured lists (small, bounded set)
local want = {
  { ext = "mp4", codec = "APVYUV422_10" },
  { ext = "mkv", codec = "ProRes422HQ" },
  { ext = "mkv", codec = "ProRes422" },
  { ext = "mkv", codec = "ProRes422LT" },
  { ext = "mkv", codec = "ProRes422P" },
  { ext = "mkv", codec = "FFV1IntraYUV422_8" },
  { ext = "mov", codec = "ProRes422HQ" },
  { ext = "mov", codec = "DNxHRHQ" },
  { ext = "mov", codec = "MPEG4" },
}

for _, w in ipairs(want) do
  local key = nil
  for k, v in pairs(fmts or {}) do
    if tostring(v):lower() == w.ext then key = k break end
  end
  if key then
    local ok = false
    local okCall, r = pcall(function() return p:SetCurrentRenderFormatAndCodec(key, w.codec) end)
    if okCall then ok = (r == true) end
    results[#results + 1] = w.ext .. ":" .. w.codec .. "=" .. tostring(ok)
  else
    results[#results + 1] = w.ext .. ":" .. w.codec .. "=nokey"
  end
end
print("VBOK|diag3|" .. table.concat(results, ","))

-- Also: does a render PRESET bypass format selection? List available presets.
local okp, presets = pcall(function() return p:GetRenderPresetList() end)
if okp and type(presets) == "table" then
  local pn = {}
  for _, pr in ipairs(presets) do
    if type(pr) == "table" then
      pn[#pn + 1] = esc(pr["PresetName"] or pr.Name or "?")
    else
      pn[#pn + 1] = esc(pr)
    end
  end
  print("VBOK|diag3|presets=" .. math.min(#pn, 20) .. "|" .. table.concat(pn, ",", 1, math.min(#pn, 20)))
else
  print("VBOK|diag3|presets=unavailable|" .. esc(presets))
end
