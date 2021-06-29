(import "../target/debug/web3.dll")
(import "../target/debug/libweb3.dylib")

(def Root (Node))

(def test
  (Chain
   "local test"
   :Looped
   (Eth "http://127.0.0.1:8545")
   (Eth.Contract :Contract "0x6b175474e89094c44da98b954eedeac495271d0f"
                 :Abi (slurp "dai.json"))

   ; Estimate gas
   "0x0" >> .args ; from addr
   1000 (BigInt) (BigInt.Shift 18) >> .args ; amount
   .args
   (Eth.EstimateGas :Method "approve"
                    :From "0x0")
   (BigInt.ToString) (Log "approve cost")

   (Clear .args)
   
   "0x0" >> .args ; addr
   .args
   (Eth.Read :Method "balanceOf")
   (Take 0) (BigInt.ToString) (Log "balance")
   (Pause 1.0)))

(schedule Root test)
(run Root 0.1)

(def test nil)
(def Root nil)