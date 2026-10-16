import { useCallback, useEffect, useState } from "react";
import { RateChart } from "../components/RateChart";
import { formatDateTime, formatExp, formatHours } from "../lib/format";
import { getMapSummaries, getSessionChart, getSessions, renameMap, type AppError, type Bar, type MapSummary, type Session } from "../lib/ipc";

const rate = (v: number | null) => (v === null ? "—" : formatExp(v));

export function HistoryView() {
  const [maps, setMaps] = useState<MapSummary[]>([]);
  const [mapId, setMapId] = useState<number | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionId, setSessionId] = useState<number | null>(null);
  const [sessionBars, setSessionBars] = useState<Bar[]>([]);
  const [editing, setEditing] = useState<{ id: number; name: string } | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  const loadMaps = useCallback(async () => {
    const res = await getMapSummaries();
    if (res.success) setMaps(res.data);
    else setError(res.error);
  }, []);

  useEffect(() => {
    void loadMaps();
  }, [loadMaps]);

  useEffect(() => {
    void getSessions(mapId).then((res) => (res.success ? setSessions(res.data) : setError(res.error)));
    setSessionId(null);
  }, [mapId]);

  useEffect(() => {
    if (sessionId === null) return setSessionBars([]);
    void getSessionChart(sessionId).then((res) => (res.success ? setSessionBars(res.data) : setError(res.error)));
  }, [sessionId]);

  const saveRename = async () => {
    if (!editing) return;
    const res = await renameMap(editing.id, editing.name);
    if (!res.success) return setError(res.error);
    setEditing(null);
    await loadMaps();
  };

  return (
    <>
      {error && <p className="error">{error.message}</p>}
      <div className="card">
        <h3>Maps</h3>
        <table>
          <thead>
            <tr><th>Map</th><th>Sessions</th><th>Training</th><th>Avg EXP/hr</th><th>Best hour</th><th /></tr>
          </thead>
          <tbody>
            {maps.map((m) => (
              <tr key={m.mapId} className={`selectable ${m.mapId === mapId ? "selected" : ""}`} onClick={() => setMapId(m.mapId === mapId ? null : m.mapId)}>
                <td>
                  {editing?.id === m.mapId ? (
                    <input value={editing.name} autoFocus onClick={(e) => e.stopPropagation()} onChange={(e) => setEditing({ id: m.mapId, name: e.target.value })} onKeyDown={(e) => e.key === "Enter" && void saveRename()} />
                  ) : (
                    m.name
                  )}
                </td>
                <td>{m.sessions}</td>
                <td>{formatHours(m.trainingSecs / 3600)}</td>
                <td>{rate(m.avgRatePerHour)}</td>
                <td>{rate(m.bestHourRate)}</td>
                <td>
                  {editing?.id === m.mapId ? (
                    <button className="primary" onClick={(e) => { e.stopPropagation(); void saveRename(); }}>Save</button>
                  ) : (
                    <button onClick={(e) => { e.stopPropagation(); setEditing({ id: m.mapId, name: m.name }); }}>Rename</button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {maps.length === 0 && <p className="status">No training sessions recorded yet.</p>}
      </div>
      <div className="card">
        <h3>{mapId === null ? "All sessions" : "Sessions on this map"}</h3>
        <table>
          <thead>
            <tr><th>Started</th><th>Map</th><th>Training</th><th>EXP</th><th>EXP/hr</th></tr>
          </thead>
          <tbody>
            {sessions.map((s) => (
              <tr key={s.id} className={`selectable ${s.id === sessionId ? "selected" : ""}`} onClick={() => setSessionId(s.id)}>
                <td>{formatDateTime(s.startedAt)}{s.endedAt === null ? " (open)" : ""}</td>
                <td>{s.mapName}</td>
                <td>{formatHours(s.trainingSecs / 3600)}</td>
                <td>{formatExp(s.expGained)}</td>
                <td>{s.trainingSecs > 0 ? formatExp((s.expGained * 3600) / s.trainingSecs) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {sessionId !== null && (
        <div className="card">
          <RateChart bars={sessionBars} />
        </div>
      )}
    </>
  );
}
