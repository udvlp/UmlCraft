// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IState extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract boolean isSubmachineState();
    public abstract void setEntry(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setDoActivity(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setExit(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.ITransition[] getInternalTransitions();
    public abstract void addInternalTransition(java.lang.String p0, java.lang.String p1, java.lang.String p2) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract java.util.List<com.change_vision.jude.api.inf.model.IState> getStateRegions();
    public abstract int getRegionSize();
    public abstract java.awt.geom.Rectangle2D getRegionRectangle(int p0) throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
    public abstract void setName(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
