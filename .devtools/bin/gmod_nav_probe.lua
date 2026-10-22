-- Server-side probe of the loaded nav mesh, run through sandcastle's luadev
-- bridge (the gmod MCP's lua_run). Set NAV_PROBE_EXPECT_BYTES to the size of
-- the .nav under test first, to check it is the file GMod resolved.
local map = game.GetMap()
local path = "maps/" .. map .. ".nav"
print("resolved_nav_bytes", file.Size(path, "GAME"), "expected", NAV_PROBE_EXPECT_BYTES)

navmesh.Load()
local areas = navmesh.GetAllNavAreas()
print("loaded", navmesh.IsLoaded(), "areas", #areas)

-- Spawns: is each on an area, and which team.
local spawns = {}
for _, ent in ipairs(ents.FindByClass("info_player_teamspawn")) do
	local area = navmesh.GetNearestNavArea(ent:GetPos(), false, 64, false, true)
	spawns[#spawns + 1] = { team = ent:GetKeyValues().TeamNum or 0, area = area }
end
local onMesh = 0
for _, s in ipairs(spawns) do
	if IsValid(s.area) then onMesh = onMesh + 1 end
end
print("spawns", #spawns, "on_mesh", onMesh)

-- Connectivity from the first spawn on the mesh, over adjacency both ways.
local start
for _, s in ipairs(spawns) do
	if IsValid(s.area) then start = s.area break end
end
local seen, queue, head = {}, { start }, 1
if start then seen[start:GetID()] = true end
while queue[head] do
	local area = queue[head]
	head = head + 1
	for _, next in ipairs(area:GetAdjacentAreas()) do
		if not seen[next:GetID()] then seen[next:GetID()] = true queue[#queue + 1] = next end
	end
	for _, prev in ipairs(area:GetIncomingConnections()) do
		if not seen[prev:GetID()] then seen[prev:GetID()] = true queue[#queue + 1] = prev end
	end
end
local reachedSpawns, teams = 0, {}
for _, s in ipairs(spawns) do
	if IsValid(s.area) and seen[s.area:GetID()] then
		reachedSpawns = reachedSpawns + 1
		teams[s.team] = true
	end
end
local teamList = {}
for t in pairs(teams) do teamList[#teamList + 1] = tostring(t) end
print("reached_areas", head - 1, "reached_spawns", reachedSpawns, "teams_reached", table.concat(teamList, ","))

-- Floor accuracy and hull clearance at every area's center.
local errors, blocked, missed = {}, 0, 0
local mins, maxs = Vector(-16, -16, 18), Vector(16, 16, 55)
for _, area in ipairs(areas) do
	local c = area:GetCenter()
	local tr = util.TraceLine({ start = c + Vector(0, 0, 32), endpos = c - Vector(0, 0, 64), mask = MASK_PLAYERSOLID })
	if tr.Hit then errors[#errors + 1] = math.abs(tr.HitPos.z - c.z) else missed = missed + 1 end
	local hull = util.TraceHull({ start = c, endpos = c, mins = mins, maxs = maxs, mask = MASK_PLAYERSOLID })
	if hull.StartSolid or hull.Hit then blocked = blocked + 1 end
end
table.sort(errors)
local function pct(p) return errors[math.max(1, math.ceil(#errors * p))] or -1 end
print(string.format("floor_error median %.2f p95 %.2f max %.2f no_floor %d", pct(0.5), pct(0.95), pct(1), missed))
print("hull_blocked_centers", blocked, "of", #areas)
