(import "../target/debug/web3.dll")
(import "../target/debug/libweb3.dylib")

(def Root (Node))

(def ganache
  (Chain
   "run-ganache"
   :Looped
   "" (Process.Run "ganache-cli" ["-d" "-f" "http://192.168.1.4:8545" ">" "_"])))

(def test
  (Chain
   "one split test"
   :Looped
   ; wait ganache
   (Pause 2.0)
   ; setup our node to default
   (Eth "http://127.0.0.1:8545")
   (Eth.Unlock "0x90F8bf6A479f320ead074411a4B0e7944Ea8c9C1")
   ; Load our contract
   (Eth.Contract :Name "one-split"
                 :Contract "0xC586BeF4a0992C495Cf22e1aeEE4E446CECDee0E"
                 :Abi (slurp "onesplit.json"))

   ; Make a constant call
   "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE" >> .args ; from token (ETH)
   "0x6b175474e89094c44da98b954eedeac495271d0f" >> .args ; to token (DAI)
   1 (BigInt) (BigInt.Shift 18) >> .args ; amount
   100  >> .args ; parts
   0 >> .args ; disable flags
   .args
   (Eth.Read :Contract .one-split
             :Method "getExpectedReturn")
   (Log) >= .res
   ; Print results
   .res (Take 0) (ExpectBytes) >= .expected
   (BigInt.ToFloat -18) (Log "price")
   .res (Take 1) (ExpectSeq) >= .distribution
   (ForEach #((ExpectBytes) (BigInt.ToFloat) (Log "dexes")))

   (Clear .args)

   1 (BigInt) (BigInt.Shift 18) (Set "options" "value")
   (Eth.GasPrice) (Set "options" "gas-price")
   "500000" (BigInt) (Set "options" "gas")
   "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE" >> .args ; from token (ETH)
   "0x6b175474e89094c44da98b954eedeac495271d0f" >> .args ; to token (DAI)
   1 (BigInt) (BigInt.Shift 18) >> .args ; amount
   .expected >> .args ; min return
   .distribution >> .args ; distribution
   0 >> .args ; flags
   .args
   (Eth.Write :Contract .one-split
              :Method "swap"
              :From "0x90F8bf6A479f320ead074411a4B0e7944Ea8c9C1"
              :Options .options
              :Confirmations 0)
   (Log)
   ; the end
   ))


(def ws-test
  (Chain
   "one split test"
   :Looped
   (Eth "ws://192.168.1.4:8546")
   (Eth.Contract :Contract "0x6b175474e89094c44da98b954eedeac495271d0f"
                 :Abi (slurp "dai.json"))
   (Eth.WaitEvent :Event "Transfer")
   (Log "Log")
   (Take "transaction_hash")
   (ExpectBytes)
   (Eth.Transaction)
   (Log "Tx")))

;; (schedule Root ganache)
;; (schedule Root test)
(schedule Root ws-test)
(run Root 0.1)

(def test nil)
(def Root nil)