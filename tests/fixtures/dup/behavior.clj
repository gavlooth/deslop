(ns fixture.dup)

(defn score-a [values]
  (let [positive (filter pos? values)
        doubled (map #(* 2 %) positive)
        adjusted (map inc doubled)
        bounded (take 10 adjusted)
        total (reduce + 0 bounded)]
    (if (> total 10)
      (+ total 3)
      (- total 1))))

(defn score-b [items]
  (let [positive (filter pos? items)
        doubled (map #(* 2 %) positive)
        adjusted (map inc doubled)
        bounded (take 10 adjusted)
        total (reduce + 0 bounded)]
    (if (> total 10)
      (+ total 3)
      (- total 1))))
