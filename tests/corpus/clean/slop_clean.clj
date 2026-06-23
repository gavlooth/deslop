(ns corpus.slop-clean)

(def domain-limit 37)

(defn uses-named-limit [input]
  (+ input domain-limit))

(defn complete-small-function [input]
  (+ input 1))

(defn documented-reason [input]
  ; Domain rule: retain the raw value for downstream reconciliation.
  input)

