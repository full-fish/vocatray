// 통계 창 — 단어장별 진도, 최근 7일 외운 단어 수, 퀴즈 정답률
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { $, el } from "./word";

type Stats = {
  books: { title: string; total: number; memorized: number }[];
  memorizedAt: string[];
  quizAsked: number;
  quizCorrect: number;
};

const percent = (part: number, whole: number) => (whole === 0 ? 0 : Math.round((part / whole) * 100));
// 내 컴퓨터 시간대의 날짜 키 — 기록 시각은 UTC라 문자열을 그대로 자르면 아침 9시 전이 전날로 잡힌다
const localDay = (d: Date) => `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`;

function render(s: Stats) {
  $("books").replaceChildren(
    ...s.books.map((b) => {
      const bar = el("div", "bar-track", "");
      const fill = el("div", "bar-fill", "");
      fill.style.width = `${percent(b.memorized, b.total)}%`;
      bar.append(fill);
      const head = el("div", "progress-head", "");
      head.append(el("span", "", b.title), el("span", "hint", `${b.memorized} / ${b.total} (${percent(b.memorized, b.total)}%)`));
      const li = el("li", "", "");
      li.append(head, bar);
      return li;
    }),
  );

  const counts = new Map<string, number>();
  for (const at of s.memorizedAt) {
    const day = localDay(new Date(at));
    counts.set(day, (counts.get(day) ?? 0) + 1);
  }
  const days = Array.from({ length: 7 }, (_, i) => {
    const d = new Date();
    d.setDate(d.getDate() - (6 - i));
    return { label: i === 6 ? "오늘" : `${d.getMonth() + 1}/${d.getDate()}`, count: counts.get(localDay(d)) ?? 0 };
  });
  const max = Math.max(1, ...days.map((d) => d.count));
  $("days").replaceChildren(
    ...days.map((d) => {
      const col = el("div", "day", "");
      const bar = el("div", "day-bar", "");
      bar.style.height = `${(d.count / max) * 100}%`;
      const track = el("div", "day-track", "");
      track.append(bar);
      col.append(el("span", "day-count", String(d.count)), track, el("span", "day-label", d.label));
      return col;
    }),
  );
  const week = days.reduce((sum, d) => sum + d.count, 0);
  $("days-note").textContent = `이번 7일 ${week}개 · 지금까지 외운 단어 ${s.memorizedAt.length}개`;

  $("quiz-summary").textContent =
    s.quizAsked === 0
      ? "아직 푼 문제가 없어요."
      : `${s.quizAsked}문제 중 ${s.quizCorrect}개 맞힘 · 정답률 ${percent(s.quizCorrect, s.quizAsked)}%`;
}

async function load() {
  render(await invoke<Stats>("stats"));
}

// 카드·퀴즈·다른 컴퓨터에서 기록이 바뀌면 다시 읽는다
listen("progress", load);
load();
