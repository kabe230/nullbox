import { useEffect, useRef, useState } from "react";
import { API_BASE_URL } from "../api/client";

export function CheckoutPage() {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const pending = useRef(false);
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const id = params.get("session_id");
    if (params.has("cancelled")) setMessage("購入はキャンセルされました。課金はありません。");
    if (!id) return;
    const controller = new AbortController();
    setMessage("テスト決済結果を確認中…");
    fetch(`${API_BASE_URL}/api/payments/checkout/${encodeURIComponent(id)}`, { signal: controller.signal })
      .then(async (r) => { if (!r.ok) throw new Error(); return r.json(); })
      .then((data) => setMessage(data.paid ? "テスト購入完了。Nullboxに戻りました。実際の課金はありません。" : "支払い完了はまだ確認できていません。"))
      .catch(() => { if (!controller.signal.aborted) setMessage("結果を確認できませんでした。再購入せず、ページを再読み込みしてください。"); });
    return () => controller.abort();
  }, []);
  async function purchase() {
    if (pending.current) return;
    pending.current = true; setBusy(true); setMessage("");
    try {
      const r = await fetch(`${API_BASE_URL}/api/payments/checkout`, { method: "POST" });
      if (!r.ok) throw new Error();
      const data = await r.json();
      const url = new URL(data.url);
      if (url.protocol !== "https:" || url.hostname !== "checkout.stripe.com") throw new Error();
      window.location.assign(url.href);
    } catch {
      setMessage("テスト決済を開始できません。設定を確認してください。");
      pending.current = false; setBusy(false);
    }
  }
  return <section>
    <h1>Nothing to See Here</h1>
    <p>大会専用のStripeテスト購入です。100円相当のテストで、実際のお金は動きません。</p>
    <p>大会指定のテストカードだけを使い、カード情報の保存は選択しないでください。</p>
    <button type="button" disabled={busy} onClick={purchase}>{busy ? "開いています…" : "テスト購入へ進む"}</button>
    <p role="status">{message}</p>
  </section>;
}
