"use client"

import * as React from "react"
import { AnimatePresence, motion, useReducedMotion, transition, collapsedScale } from "@/motion"
import { useIsPresent } from "motion/react"
import { currentOrigin, originElement, type MotionOrigin } from "@/motion/origin"
import { cn } from "@/lib/utils"
import { XIcon } from "lucide-react"
import { Dialog as DialogPrimitive } from "radix-ui"

import { Button } from "@/components/ui/button"

const DialogMotion = React.createContext<{open:boolean;origin:MotionOrigin|null;opener:HTMLElement|null}>({open:false,origin:null,opener:null})
function Dialog({ open:controlled, defaultOpen=false, onOpenChange, ...props }:React.ComponentProps<typeof DialogPrimitive.Root>) {
  const [internal,setInternal]=React.useState(defaultOpen)
  const open=controlled??internal
  const wasOpen=React.useRef(false)
  const anchor=React.useRef<{origin:MotionOrigin|null;opener:HTMLElement|null}>({origin:null,opener:null})
  if(open&&!wasOpen.current)anchor.current={origin:currentOrigin(),opener:originElement()}
  wasOpen.current=open
  return <DialogMotion.Provider value={{open,...anchor.current}}><DialogPrimitive.Root data-slot="dialog" open={open} onOpenChange={value=>{setInternal(value);onOpenChange?.(value)}} {...props}/></DialogMotion.Provider>
}

function DialogTrigger({
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Trigger>) {
  return <DialogPrimitive.Trigger data-slot="dialog-trigger" {...props} />
}

function DialogPortal({
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Portal>) {
  return <DialogPrimitive.Portal data-slot="dialog-portal" {...props} />
}

function DialogClose({
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Close>) {
  return <DialogPrimitive.Close data-slot="dialog-close" {...props} />
}

function DialogOverlay({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Overlay>) {
  return (
    <DialogPrimitive.Overlay
      data-slot="dialog-overlay"
      className={cn(
        "fixed inset-0 z-50 bg-black/50 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:animate-in data-[state=open]:fade-in-0",
        className
      )}
      {...props}
    />
  )
}

function DialogSurface({origin,opener,...props}:React.ComponentProps<typeof motion.div>&{origin:MotionOrigin|null;opener:HTMLElement|null}) {
  const reduced=!!useReducedMotion()
  const present=useIsPresent()
  const target=!present&&opener?.isConnected?opener.getBoundingClientRect():origin
  const from={opacity:0,x:reduced?0:target?(target.x+target.width/2-innerWidth/2)*(1-collapsedScale):0,y:reduced?0:target?(target.y+target.height/2-innerHeight/2)*(1-collapsedScale):12,scale:reduced?1:collapsedScale}
  return <motion.div {...props} data-motion="dialog" data-motion-state={present?'open':'closed'} inert={!present} initial={from} animate={{opacity:1,x:0,y:0,scale:1}} exit={from} transition={transition('window',reduced)}/>
}
function DialogContent({className,children,showCloseButton=true,onCloseAutoFocus,...props}:React.ComponentProps<typeof DialogPrimitive.Content>&{showCloseButton?:boolean}) {
  const {open,origin,opener}=React.useContext(DialogMotion)
  const reduced=!!useReducedMotion()
  return <AnimatePresence propagate>{open&&<DialogPortal forceMount><DialogPrimitive.Overlay forceMount asChild><motion.div data-slot="dialog-overlay" className="fixed inset-0 z-50 bg-black/40" initial={{opacity:0}} animate={{opacity:1}} exit={{opacity:0}} transition={transition('window',reduced)}/></DialogPrimitive.Overlay>
    <DialogPrimitive.Content forceMount asChild onCloseAutoFocus={event=>{onCloseAutoFocus?.(event);if(!event.defaultPrevented&&opener?.isConnected){event.preventDefault();opener.focus()}}} {...props}>
      <DialogSurface origin={origin} opener={opener} data-slot="dialog-content" className={cn("fixed top-1/2 left-1/2 z-50 grid w-full max-w-[calc(100%-2rem)] -translate-x-1/2 -translate-y-1/2 gap-4 rounded-lg border bg-background p-6 shadow-lg outline-none sm:max-w-lg",className)}>
        {children}
        {showCloseButton&&<DialogPrimitive.Close data-slot="dialog-close" className="absolute top-4 right-4 rounded-xs opacity-70 hover:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"><XIcon className="size-4"/><span className="sr-only">关闭</span></DialogPrimitive.Close>}
      </DialogSurface>
    </DialogPrimitive.Content>
  </DialogPortal>}</AnimatePresence>
}

function DialogHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="dialog-header"
      className={cn("flex flex-col gap-2 text-center sm:text-left", className)}
      {...props}
    />
  )
}

function DialogFooter({
  className,
  showCloseButton = false,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  showCloseButton?: boolean
}) {
  return (
    <div
      data-slot="dialog-footer"
      className={cn(
        "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end",
        className
      )}
      {...props}
    >
      {children}
      {showCloseButton && (
        <DialogPrimitive.Close asChild>
          <Button variant="outline">关闭</Button>
        </DialogPrimitive.Close>
      )}
    </div>
  )
}

function DialogTitle({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Title>) {
  return (
    <DialogPrimitive.Title
      data-slot="dialog-title"
      className={cn("text-lg leading-none font-semibold", className)}
      {...props}
    />
  )
}

function DialogDescription({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description
      data-slot="dialog-description"
      className={cn("text-sm text-muted-foreground", className)}
      {...props}
    />
  )
}

export {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
}
